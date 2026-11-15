# M1 Tasarım Notu — Çekirdek

Kapsam: `wish_core`, `wish_schema`, `wish_ecs`, `wish_app`, `wish_cli`.
Çıkış ölçütleri: [ROADMAP.md](../ROADMAP.md) M1 bölümü. İlgili kararlar: ADR-003, 004, 005, 006.

Bu not M1'in **nasıl** yapılacağını sabitler. Burada verilen kararlar uygulama sırasında yanlış çıkarsa önce bu not güncellenir, sonra kod.

## 1. Temel karar: bileşenler düz veridir

Dünyaya giren her tür (bileşen, kaynak, olay) **düz veridir**: `Copy`, sabit boyutlu, işaretçi içermez, yıkıcısı (drop) yoktur.

Sonuçları:

| Sonuç | Neden önemli |
|---|---|
| Anlık görüntü = bayt kopyası | Geri sarma, çatallama ve tekrar ucuz (ADR-006) |
| Ev sahibi ile Wasm konuğu aynı baytları görür | Dönüştürme katmanı yok (ADR-011) |
| Depolama yalnızca bayt dizileridir | `unsafe` yüzeyi küçük, yıkıcı hatası sınıfı yok |
| Dünya hash'i veri üzerinde tanımlıdır | Determinizm doğrulanabilir (ADR-005) |

### M1'de desteklenen alan türleri

| Tür | Boyut / hizalama | JSON gösterimi |
|---|---|---|
| `bool` | 1 / 1 | `true` / `false` |
| `i8`…`i64`, `u8`…`u64` | 1–8 / 1–8 | sayı |
| `f32`, `f64` | 4, 8 | sayı (sonlu olmalı) |
| `Vec2`, `Vec3`, `Vec4` | 8, 12, 16 / 4 | `[x, y]`, `[x, y, z]`, `[x, y, z, w]` |
| `Quat` | 16 / 4 | `[x, y, z, w]` |
| `Mat4` | 64 / 4 | 16 sayılık dizi (sütun öncelikli) |
| `Color` (doğrusal RGBA, `f32`) | 16 / 4 | `[r, g, b, a]` |
| `Entity` | 8 / 4 | çalışma zamanında `"12v3"`; sahne dosyasında sahne kimliği (M2) |
| `Text` (dünya metin tablosuna tutamaç) | 4 / 4 | metin |
| C benzeri `enum` (`repr(u8)`) | 1 / 1 | varyant adı |
| `[T; N]` | N × T | dizi |
| İç içe şemalı yapı | — | nesne |

**En büyük hizalama 8'dir**; daha büyüğünü şema reddeder. Bu, sütunların `Vec<u64>` üzerine kurulmasını sağlar (§4).

**Bilinçli olarak ertelenenler:** `List<T>` (değişken uzunluk), `Opt<T>`, veri taşıyan enum'lar, `AssetRef` (M2'de varlık sunucusuyla), `Mat3`. Hiyerarşi liste gerektirmez (§4.6).

### Matematik türleri

`glam`, **`scalar-math`** ve **`libm`** özellikleriyle kullanılır (özellik adları WP3'te `cargo` ile doğrulanacak):

- `scalar-math`: SIMD kapalı; `Quat`/`Vec4`/`Mat4` düz `f32` yapılarıdır (hizalama 4). SIMD'li `Quat` 16 hizalamalıdır ve yapılarda dolguya yol açar.
- `libm`: trigonometri vb. platformun C kitaplığından değil saf Rust'tan gelir; platformlar arası determinizm hedefine (D2) yaklaştırır.

Bedeli: simülasyon matematiği SIMD kullanmaz. Render tarafı gerekirse kendi SIMD yolunu kullanır (determinizm sınırının dışı). M1 kıyaslaması bunun yeterli olup olmadığını gösterecek.

## 2. Depolama yerleşimi

**Karar: depolama yerleşimi, türün `#[repr(C)]` Rust yerleşimidir.** Sütunlar bayt dizisidir; yerel Rust kodu bunlara dönüştürmesiz `&T` / `&mut T` olarak bakar.

Değerlendirilen seçenekler:

| Seçenek | Artı | Eksi | Sonuç |
|---|---|---|---|
| **A. `repr(C)` yerleşimi = depolama** | Sıfır kopya erişim; tanıdık sorgu API'si; `offset_of!` ile şema bedavaya | Dolgu baytları belirsiz içerik taşıyabilir | **Seçildi** |
| B. Şemanın kendi sıkıştırılmış yerleşimi | Dolgu yok; ham bayt hash'i her zaman geçerli | Her erişimde alan alan kopyalama; `&mut T` yerine geri yazan sarmalayıcılar | Reddedildi |
| C. Dolguyu yasakla (bytemuck `Pod` gibi) | Ham bayt hash'i her zaman geçerli | Ajan/insan alan sırasıyla uğraşır, `bool` ve enum kullanılamaz | Reddedildi |

**`repr(C)` yerleşimi platformlar arasında aynı mı?** İzin verilen türler için evet: x86_64, aarch64 ve wasm32'de `u64`/`f64` 8 hizalamalıdır; `usize`, işaretçi ve `u128` yasaktır. Şema ofsetleri açıkça taşır; konuk SDK'sı yüklenirken kendi yerleşimini şemayla karşılaştırır, uyuşmazlık tanı üretir.

### Dolgu baytları

A seçeneğinin bedeli. İki kural:

1. **Dolgu hiçbir zaman yorumlanmaz.** Anlık görüntü dolguyu ham kopyalar (içeriğine bakmaz).
2. **Hash dolguyu atlar.** Dünya hash'i şemayı izleyerek alan alan hesaplanır. Dolgusu olmayan ve `Text` içermeyen türler kayıt sırasında işaretlenir (`raw_hashable`) ve sütunları tek çağrıda ham hash'lenir — hızlı yol. `Transform` (Vec3 + Quat + Vec3 = 40 bayt, dolgusuz) hızlı yoldadır.

### Geçerlilik (güven sınırı)

`bool` yalnızca 0/1, enum yalnızca tanımlı varyant değeri taşıyabilir; aksi hâlde yerel `&T` görünümü tanımsız davranış olur. Bu yüzden **güvenilmeyen baytlar sütuna girmeden önce şemayla doğrulanır**: JSON'dan gelen değerler (yapı gereği geçerli) ve ileride Wasm konuğunun geri yazdığı sütunlar (M5). Yerel Rust kodunun `&mut T` üzerinden yazdıkları yapı gereği geçerlidir.

## 3. `wish_schema`

### Veri modeli

```rust
pub struct TypeSchema {
    pub name: String,          // "Transform"
    pub doc: String,
    pub size: u32,
    pub align: u32,
    pub kind: TypeKind,
}

pub enum TypeKind {
    Bool, I8, I16, I32, I64, U8, U16, U32, U64, F32, F64,
    Vec2, Vec3, Vec4, Quat, Mat4, Color,
    Entity, Text,
    Enum(Vec<VariantSchema>),            // ad, değer, belge
    Array { elem: Box<TypeSchema>, len: u32 },
    Struct(Vec<FieldSchema>),
}

pub struct FieldSchema {
    pub name: String,
    pub doc: String,
    pub offset: u32,
    pub ty: TypeSchema,
    pub unit: Option<String>,            // "m/s"
    pub range: Option<(f64, f64)>,       // kapalı aralık
    pub default: Option<serde_json::Value>,
}
```

### Sözleşme ve makro

```rust
/// # Safety
/// Tür `repr(C)`, `Copy`, işaretçisiz olmalı ve `schema()` gerçek yerleşimi anlatmalıdır.
pub unsafe trait Plain: Copy + 'static {
    fn schema() -> TypeSchema;
    fn default_value() -> Self;              // varsayılanın tek kaynağı
    fn encode(&self, out: &mut [u8]);        // güvenli; dolguyu sıfır yazar
    fn decode(bytes: &[u8]) -> Option<Self>; // güvenli; geçersiz bool/enum → None
}
pub trait Component: Plain { const NAME: &'static str; }
pub trait Resource: Plain { const NAME: &'static str; }
pub trait Event: Plain { const NAME: &'static str; }
```

`#[component]` (ve `#[resource]`, `#[event]`, iç içe yapılar/enum'lar için `#[schema]`) öznitelik makrosu şunları üretir:

- `#[repr(C)]` + `Clone, Copy` (enum'da `repr(u8)`)
- `unsafe impl Plain` — ofsetler `core::mem::offset_of!`, boyut/hizalama `size_of`/`align_of` ile; elle yazılan sayı yok
- `Debug`, `PartialEq` (enum'da ayrıca `Eq`) ve `impl Default`. Varsayılan, `#[schema(default = …)]` ifadelerinden gelir; **herhangi bir Rust ifadesi** olabilir (`default = Vec3::ONE`). Şemadaki JSON varsayılanı elle yazılmaz: Rust değeri `encode` edilip şemayla JSON'a çevrilerek hesaplanır — tek doğruluk kaynağı.
- Derleme zamanı denetimleri: her alan türü `Plain`; hizalama ≤ 8
- `///` yorumları → `doc`; `#[schema(unit = …, range = …, alias = …)]` → şema
- `alias`: insanların/ajanların gerçek ad yerine deneyeceği ad (`translation` için `position`). Girdi olarak **kabul edilmez**; yalnızca "şunu mu demek istediniz" ipucunu kesinleştirir (tek doğru yol ilkesi)
- Alansız yapılar (`struct Player;`) geçerlidir: sıfır boyutlu "etiket" bileşenleri
- Enum'un varsayılanı ilk varyantıdır

**Neden `encode`/`decode`?** Güvenli, alan alan çalışan bu iki yöntem sayesinde: varsayılanlar tek kaynaktan türer; bileşen ekleme/okuma `unsafe` gerektirmez (`unsafe` yalnızca sıfır kopya `&T` görünümlerine kalır); güvenilmeyen baytlar `decode` ile tür güvenli doğrulanır. Sıcak yol (sorgu gezinmesi) bunları kullanmaz.

Makro ayrı bir crate'tir (`wish_schema_macros`), çünkü Rust'ta işlemsel makrolar ayrı crate olmak zorundadır.

### JSON ⇄ bayt

Tamamen güvenli kodla (bayt dilimleri üzerinde `from_le_bytes`/`to_le_bytes`), şemayı izleyerek:

- Eksik alan → şema varsayılanı (yoksa sıfır).
- Bilinmeyen alan → `unknown_field` tanısı, "şunu mu demek istediniz" önerisiyle.
- Aralık dışı sayı → `value_out_of_range`; sonlu olmayan sayı → `non_finite_number`.
- `Text` alanları için dönüştürücü bir metin deposu bağlamı alır.

### JSON Schema ve parmak izi

- Her tür için JSON Schema (2020-12): `description`, `minimum`/`maximum`, `default`, `x-unit`, `additionalProperties: false`.
- **Kayıt parmak izi:** tüm kayıtlı şemaların (ad, tür, ofset, boyut) kararlı hash'i. Anlık görüntüler bu izi taşır; farklı tür kümesine geri yükleme reddedilir.

## 4. `wish_ecs`

### 4.1 Durum ve kayıt ayrımı

```rust
pub struct World {
    registry: Registry,     // tür bilgisi; başlangıçtan sonra yalnızca eklenir
    state: WorldState,      // tüm dinamik durum
}
#[derive(Clone)]
struct WorldState { tick, entities, archetypes, resources, events, texts, rng }
```

**Anlık görüntü `WorldState`'in kopyasıdır.** Bu ayrım, "anlık görüntüye girmeyi unutulan durum" hatasını yapısal olarak önler: dinamik olan her şey tek yapının içindedir.

### 4.2 Varlıklar

`Entity { index: u32, generation: u32 }` (`wish_core`'da; şema da kullandığı için). Boş varlık `Entity::NULL`. Ayırıcı: kuşak dizisi + **LIFO** serbest liste (deterministik). Varlık → (arketip, satır) konum tablosu.

### 4.3 Arketip tabloları

- Arketip = sıralı `ComponentId` kümesi. Arketipler **oluşturulma sırasıyla** numaralanır ve o sırayla gezilir.
- Arketip araması `BTreeMap<Vec<ComponentId>, ArchetypeId>` (hash haritası üzerinde gezinme yok).
- Sütun: `Column { words: Vec<MaybeUninit<u64>>, item_size, len }`. `u64` sözcükleri 8 hizalamayı güvenli biçimde garanti eder; elle bellek ayırma (`alloc`) yok. `MaybeUninit` olmasının nedeni §12 WP4 notunda.
- Her sütunda satır başına `changed_tick: u32` (değişiklik izleme). Değişiklik işaretleri sistem davranışını etkilediği için durumun parçasıdır ve hash'e girer.
- Silme: `swap_remove` (deterministik; aynı işlem dizisi aynı satır sırasını üretir).

### 4.4 Erişim

- **Türlü sorgu:** `world.query::<(Entity, &Position, &mut Velocity)>()`, süzgeçler `With<T>` / `Without<T>`. Sorgu kurulurken erişim denetlenir (aynı bileşene çakışan yazma → tanı; kayıtsız bileşen → tanı). `&mut T` erişimi `Mut<T>` sarmalayıcısıyla verilir; yalnızca gerçekten yazıldığında değişiklik işareti konur.
- **Dinamik erişim:** `ComponentId` ile bayt dilimi. JSON/MCP ve ileride Wasm bu yolu kullanır.
- **Yapısal değişiklikler:** doğrudan (`world.spawn`, `insert`, `remove`, `despawn`) ya da gezinme sırasında `Commands` tamponuyla; tampon her sistemden sonra **kayıt sırasıyla** uygulanır.

### 4.5 Kaynaklar, olaylar, metinler

- **Kaynak:** türü başına tek değer, düz veri, bayt olarak saklanır.
- **Olay:** düz veri; çift tamponlu. N. tick'te yazılan olay N. tick'in kalanında ve N+1. tick'te okunur. Tamponlar durumun parçasıdır.
- **Metin tablosu:** yalnızca eklenen, yinelenenleri birleştiren tablo; `Text(0)` boş metindir. Hash metnin içeriği üzerinden hesaplanır, tutamaç numarası üzerinden değil.

### 4.6 Hiyerarşi

Liste gerektirmeyen bağlı yapı, tek bileşen:

```rust
#[component]
pub struct Hierarchy { parent: Entity, first_child: Entity, last_child: Entity,
                       prev_sibling: Entity, next_sibling: Entity }
```

Yalnızca `world.set_parent` / `world.remove_parent` üzerinden değişir (alanlar dışarıya salt okunur). Çocuk sırası ekleme sırasıdır. Döngü oluşturma girişimi tanı üretir. Varlık silinince alt ağacı da silinir.

## 5. Zaman, zamanlama, rastgelelik

- **Tick:** `Time { tick: u64, delta: f32 }` kaynağı; varsayılan 60 Hz. Simülasyon kodu duvar saatine erişemez.
- **Sistem (M1):** `name` + `fn(&mut World) -> Result<()>` + `before`/`after` kısıtları. Hata döndüren sistem devre dışı kalır, tanı kaydedilir, dünya ilerler (M5'teki Wasm tuzak davranışıyla aynı).
- **Sıra:** kısıtlara göre topolojik sıralama; hazır düğümler arasında **ada göre en küçük** seçilir. Sonuç kayıt sırasından bağımsız tek bir toplam sıradır. Döngü → tanı (döngüdeki sistem adlarıyla).
- **Yürütme:** tek iş parçacığı, sıralı. Paralellik M1 kapsamı dışıdır; eklendiğinde aynı hash'i üretmek zorundadır.
- **Rastgelelik:** kendi içimizde yazılan `xoshiro256++`, tohumlama `SplitMix64` (dış crate sürümü değişince dizinin değişmesi riski yok; başvuru vektörleriyle test edilir). Her sistemin kendi akışı vardır: `akış = f(dünya tohumu, sistem adının kararlı hash'i)`. Akış durumları `WorldState` içindedir.

Bilinçli erteleme: Bevy tarzı parametre enjeksiyonu (`fn sys(q: Query<…>, t: Res<Time>)`). M1'de sistemler `&mut World` alır; ergonomik katman, erişim bildirimiyle birlikte M5'te konuk SDK'sı tasarlanırken eklenir.

## 6. Anlık görüntü ve dünya hash'i

**Anlık görüntü:** `world.snapshot() -> Snapshot` (`WorldState` kopyası + kayıt parmak izi). `world.restore(&snapshot)` parmak izi uyuşmazsa tanı döndürür. Yalnızca tick sınırında alınır. M1'de bellektedir; diske yazım biçimi M4/M5'te.

**Dünya hash'i:** `world.hash() -> Hash128` (XXH3-128). Sırasıyla: parmak izi, tick, varlık ayırıcı (kuşaklar + serbest liste), her arketip (bileşen kimlikleri, satır sayısı, varlık sütunu, her sütunun verisi ve değişiklik işaretleri), kaynaklar, olay tamponları, rastgelelik akışları. Bölümler etiket ve uzunlukla ayrılır.

Ne garanti eder: **aynı hash ⇒ aynı gelecek** (aynı girdilerle). Bu yüzden gelecekteki davranışı etkileyen her şey (serbest liste sırası dahil) hash'e girer.

Ne garanti **etmez**: farklı geçmişlerle ulaşılmış, "anlamca aynı" iki dünyanın aynı hash'i vermesi. Bu bir durum hash'idir, anlamsal eşitlik değil. Sahne dosyası düzeyindeki eşitlik M2'nin kanonik biçimiyle sağlanır.

## 7. `unsafe` sınırı

| Yer | Ne için |
|---|---|
| `wish_schema` | Yalnızca `unsafe trait Plain` bildirimi (sözleşme) |
| `wish_schema_macros` çıktısı | `unsafe impl Plain` (üretilmiş kod, ofsetler derleyiciden) |
| `wish_ecs` | Bayt sütunu ⇄ `&T`/`&mut T` dönüşümü, sorgu yineleyicileri |

Diğer her yerde `unsafe_code = "deny"` (çalışma alanı lint'i). **CLAUDE.md'deki kural bu nota göre güncellendi:** `wish_schema` sözleşme bildirimi için listeye eklendi.

Güvence: (1) her `unsafe` bloğunda `// SAFETY:` gerekçesi; (2) `wish_ecs` testleri Miri altında koşar (nightly araç zinciri gerekir, WP4'te kurulur); (3) model tabanlı özellik testleri.

## 8. Komut satırı (M1)

M1'de sahne dosyası (M2) ve betik (M5) olmadığından `wen`, yerleşik örnek dünyaları çalıştırır:

```
wen run --example bounce --headless --frames 10000 --seed 7 --json
  → { "tick": 10000, "world_hash": "…", "entities": 1000, "elapsed_ms": … }
wen schema --json          → kayıtlı tüm türlerin JSON Schema'sı
wen bench ecs --entities 100000 --ticks 600
```

`elapsed_ms` duvar saatidir ve yalnızca komut satırı katmanında ölçülür; simülasyona girmez.

## 9. Test planı

| Çıkış ölçütü | Test |
|---|---|
| İki çalıştırma aynı hash | Örnek dünya 10.000 tick × 2; her 100 tick'te hash karşılaştırması |
| Anlık görüntü → geri yükle → devam = kesintisiz | 5.000. tick'te anlık görüntü; iki yol 10.000. tick'te karşılaştırılır |
| JSON gidiş-dönüşü kayıpsız | Özellik testi: rastgele değer → JSON → bayt → JSON |
| JSON Schema üretimi | Örnek türler için altın çıktı |
| Miri temiz | `wish_ecs` birim testleri, küçük boyutlarla |
| ECS doğruluğu | Model tabanlı özellik testi: rastgele spawn/despawn/insert/remove/set dizileri basit bir başvuru modeliyle karşılaştırılır |
| Sıralama | Kayıt sırası karıştırılır, yürütme sırası değişmez; döngü tanı üretir |
| Rastgelelik | Başvuru vektörleri; akışların bağımsızlığı |
| Kıyaslama | 100 bin varlık, basit hareket sistemi, tick süresi (release) kaydedilir |

## 10. İş paketleri ve önerilen model

| # | İş paketi | Model · efor | Durum |
|---|---|---|---|
| WP1 | Çalışma alanı, lint'ler, lisanslar, CI, git | Sonnet 5.5 · medium | Tamamlandı |
| WP2 | `wish_core`: `Entity`, `Diagnostic`, kararlı hash, öneri yardımcıları | Sonnet 5.5 · medium | Tamamlandı |
| WP3 | `wish_schema` + makro: şema modeli, JSON ⇄ bayt, JSON Schema, parmak izi | Opus 5.5 · high | Tamamlandı |
| WP4 | `wish_ecs`: depolama, türlü ve dinamik erişim, sorgular, kaynaklar, metin tablosu | Opus 5.5 · high | Tamamlandı |
| WP4b | `wish_ecs`: ertelenmiş komutlar, olaylar, hiyerarşi, değişiklik izleme | Opus 5.5 · high | Tamamlandı |
| WP5 | Zamanlayıcı, sabit adım, rastgelelik (`wish_app`) | Opus 5.5 · high | |
| WP6 | Anlık görüntü ve dünya hash'i | Opus 5.5 · high | |
| WP7 | `wish_cli`: `wen run`, `wen schema`, `wen bench` | Sonnet 5.5 · medium | |
| WP8 | Determinizm testleri, Miri, kıyaslama, M1 kapanış incelemesi | Opus 5.5 · high | |

## 11. Açık sorular (uygulama sırasında kapanacak)

1. ~~**`f32` JSON gidiş-dönüşü.**~~ **WP3'te kapandı.** Yazarken `f32`'nin en kısa ondalık gösterimi kullanılır (JSON'da `0.1` görünür, `0.10000000149011612` değil); okurken sayının ondalık metni doğrudan `f32` olarak ayrıştırılır (çift yuvarlama yok). Ayrıca `serde_json`'ın `float_roundtrip` özelliği açıldı: varsayılan ayrıştırıcı `f64`'te son basamakta sapabiliyordu. Özellik testleri her sonlu `f32` ve `f64` için bayt → JSON → metin → JSON → bayt yolunun bit düzeyinde aynı sonucu verdiğini doğruluyor.
2. **Devre dışı kalan sistem bayrakları** durumun parçası mı? M1'de zamanlayıcıdadır (anlık görüntü dışı). M5'te Wasm tuzaklarıyla birlikte yeniden değerlendirilir.
3. **Metin tablosunun büyümesi.** Yalnızca eklenen tablo uzun oturumlarda şişebilir; sıkıştırma (anlık görüntü sırasında) gerekirse M5'te.
4. **`scalar-math` performansı.** WP8 kıyaslaması yetersiz çıkarsa: depolamada düz `f32` yapıları, hesaplamada SIMD türlerine dönüştürme.
5. **Makronun yinelenen hataları.** Düz veri olmayan bir alan (`String` gibi) doğru yerde, doğru iletiyle raporlanıyor; ama aynı hata üretilen kodun başka üç yerinden de yineleniyor (bir hata için beş ileti). İlk ileti yeterince açık; yinelemeleri azaltmak ve derleme hatası testleri (`trybuild`) eklemek WP8'e bırakıldı.
6. **`syn` sürümü.** Makro `syn` 2 ile yazıldı; `serde_derive` ise `syn` 3 getiriyor. İki sürüm de derleniyor (ilk derlemede birkaç saniye). Makro ileride `syn` 3'e taşınabilir.

## 12. Uygulama notları

Tasarımdan sonra uygulama sırasında verilen ve sonraki iş paketlerini etkileyen kararlar.

### WP2–WP3

- **`Diagnostic` tek işaretçi genişliğindedir.** İçeriği (`DiagnosticData`) yığında (heap) durur; alanlara `Deref` ile doğrudan erişilir (`diagnostic.code`). Neden: ilk hâli 200 bayttı ve her `Result<T>`'yi, hata olmasa bile, o kadar büyütüyordu. Şimdi `Result<()>` 8 bayt. Bunu bir test sabitliyor.
- **Sonlu olmayan sayılar.** Okumada reddedilir. Yazmada `"NaN"`, `"Infinity"`, `"-Infinity"` metinleri üretilir; böylece bozulmuş bir dünya (örneğin NaN'a dönmüş bir konum) yine de incelenebilir.
- **`apply_json` bir yamadır.** Nesnede adı geçen alanlar yazılır, diğerleri olduğu gibi kalır. Yeni değer kurmak için `from_json` (varsayılanlardan başlar). MCP'nin `set` işlemi doğrudan bunun üstüne kurulacak.
- **Hata konumu ve adı.** JSON dönüşüm hataları hem JSON işaretçisi (`/waypoints/1/1`) hem okunur ad (`Mover.waypoints[1][1]`) taşır. Çağıran, değerin belge içindeki yerini `JsonContext::at_path` ile verir.
- **Şema doğrulaması** (`TypeSchema::validate`) makronun ürettiği şemalar için hep geçer; asıl amacı M5'te Wasm modülünden gelecek şemaları güven sınırında denetlemektir.
- **Tür düzeyi varsayılanlar** iki yerde tanımlı ve bir testle eşitlenmiş durumda: Rust tarafında `Plain::default_value()`, şema tarafında `default_bytes` (dördey = birim, matris = birim, renk = opak beyaz, varlık = `null`, enum = ilk varyant, diğerleri sıfır).
- **Parmak izi yalnızca yerleşimi kapsar** (ad, boyut, hizalama, ofset, enum değerleri). Belge, birim, aralık, varsayılan ve takma ad değişince parmak izi değişmez; çünkü saklı baytların anlamı değişmez.

### WP4

- **Sütunlar `MaybeUninit` saklar; dolgu hiçbir zaman bayt olarak okunmaz.** §2'deki ilk plan "dolgu her zaman sıfırdır" varsayıyordu. Bu yanlıştı: güvenli kod `&mut T` üzerinden bütün bir değer atadığında (`*t = Health { .. }`) derleyici dolgu baytlarını başlatılmamış bırakabilir, ve başlatılmamış baytı `u8` olarak okumak tanımsız davranıştır. Dolguyu sonradan sıfırlamak da güvenli değildi: `mem::forget` güvenli koddur ve sıfırlama kodunu atlatır. Çözüm:
  - Kayıt her tür için **alan aralıklarını** (`spans`) hesaplar: dolgu dışındaki bayt aralıkları, birleştirilmiş.
  - Bayt olarak okuma (`component_bytes`, JSON, ileride dünya hash'i) yalnızca bu aralıkları kopyalar; dolgu sıfır çıkar.
  - Tüm değer kopyaları (arketipler arası taşıma, `swap_remove`, ileride anlık görüntü) ham bellek kopyasıdır; `MaybeUninit` için bu tanımlıdır.
  - Böylece §2'deki iki kural korunur: dolgu yorumlanmaz; hash dolguyu atlar.
- **Tüm erişim `&mut World` üzerinden.** Sorgular da dahil (`world.query::<Q>()` dünyayı değişken ödünç alır). Tek yazıcı garantisi derleyiciden gelir; çalışma zamanı ödünç denetimi yok. Paralel sistemler eklendiğinde (M1 sonrası) salt okunur sorgular için `&World` yolu eklenecek.
- **Sorgu erişim denetimi `TypeId` ile yapılır**, bileşen kimliğiyle değil: böylece `(&mut A, &A)` gibi hatalı bir sorgu, `A` hiç kaydedilmemiş olsa bile `conflicting_query_access` tanısıyla reddedilir.
- **Kaydedilmemiş bileşen** türlü sorguda hiçbir şeyle eşleşmez (hata değil); `With<T>` boş, `Without<T>` koşulsuz geçer. Adla erişimde ise (`set_json`) `unknown_component` tanısı ve en yakın adın önerisi döner.
- **`set_json` atomiktir.** Değer önce bir kopya üzerinde uygulanır; reddedilirse dünya değişmez. Bileşen yoksa varsayılanlardan başlar, varsa yama uygular.
- **Kaynaklar** tek satırlık sütunlardır; aynı türlü, JSON ve bayt yolları kullanılır.
- **WP4b'ye ertelenenler:** ertelenmiş komut tamponu (`Commands`), olaylar, hiyerarşi bileşeni, değişiklik izleme (`changed_tick` ve `Mut<T>` sarmalayıcısı). Bunlar zamanlayıcıyla (WP5) birlikte anlam kazanıyor; ayrı ve küçük bir iş paketi olarak daha güvenli.
- **Toplu ekleme yok.** Bir varlığa birden çok bileşen eklemek her ekleme için bir arketip taşıması yapar. 100 bin varlıklı kıyaslama (WP8) yetersiz gösterirse demet tabanlı `spawn_with` eklenecek.

### WP4b

- **Değişiklik işaretleri `u64`.** Her sütun satır başına iki işaret tutar: eklendiği ve son yazıldığı "değişiklik tick'i". `u32` 60 Hz'de sistem başına artışla birkaç haftada taşardı; `u64` taşmaz ve karşılaştırmalar basit kalır. Dünya tick'i 1'den başlar; 0 her şeyden öncedir.
- **Değişiklik tick'i ile simülasyon tick'i ayrı kavramlardır.** Değişiklik tick'i yalnızca "şundan sonra mı yazıldı?" sorusu içindir; zamanlayıcı (WP5) onu her sistemden sonra ilerletecek.
- **`&mut T` artık `Mut<T>` verir.** Okumak işaret koymaz; yazmak (`DerefMut`) koyar. Bu yüzden döngüde `for mut position in world.query::<&mut Position>()?` yazılır. `Changed<T>` ve `Added<T>` satır düzeyinde süzgeçlerdir; satırın işaretini öğesi oluşturulmadan önce okur. Karşılaştırılacak tick `query_since` ile verilir; `query_filtered` 0'ı kullanır.
- **Olaylar** tür başına çift tamponludur: bir güncellemede gönderilen olay o güncellemede ve sonrakinde okunur; `update_events` eski yarıyı atar. Her olayın tür içinde sıra numarası vardır; `EventCursor` ile okuyan her olayı bir kez görür, geç kalan okuyucu atılanları atlar.
- **Komutlar** (`Commands`) sırayla uygulanır; başarısız olan komut, buffer içindeki sırası (`location.op`) ile raporlanır ve diğerleri yine çalışır. Aynı buffer'da oluşturulacak bir varlığa `Target::Spawned(n)` ile başvurulur. Türlü ekleme, türü bilen bir işlev işaretçisi saklar; böylece komut kaydedilirken dünyaya erişim gerekmez.
- **Hiyerarşi**, tek bir bileşende (`Hierarchy`: ebeveyn, ilk/son çocuk, önceki/sonraki kardeş) çift bağlı liste olarak durur; liste türü gerekmez, düz veri kalır. Bileşen **motor tarafından yönetilir**: okunabilir (JSON dahil), ama doğrudan yazma, ekleme, silme, `get_mut` ve `&mut` sorgusu `managed_component` tanısıyla reddedilir; değişiklik yalnızca `set_parent` / `remove_parent` ile yapılır. Döngü oluşturma `hierarchy_cycle` ile reddedilir. Bağlantıları izleyen her yürüyüş varlık sayısıyla sınırlıdır; bozuk bağlantılar motoru kilitleyemez.
- **`despawn` alt ağacı da siler.** Model tabanlı test rastgele `set_parent` / `remove_parent` / `despawn` dizileriyle bunu ve kardeş sırasını doğruluyor.
- **Öneri algoritması iyileştirildi** (`wish_core::suggest`): iki komşu harfin yer değiştirmesi artık tek hata sayılıyor (`Hti` → `Hit`). Bunu yeni olay testi yakaladı.
