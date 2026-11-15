# M2 Tasarım Notu — Veri Katmanı

Kapsam: `wish_scene` (sahne ve prefab belgeleri, kanonik biçim, doğrulama, işlemler, geri al), `wish_asset` (varlık dosyaları, izleme, `*.meta.json`), `wen check / fmt / mv / migrate`.
Çıkış ölçütleri: [ROADMAP.md](../ROADMAP.md) M2 bölümü. İlgili kararlar: ADR-007 (dosya formatları), ADR-008 (dosyalar doğruluk kaynağıdır), ADR-018 (hibrit: tek geçmiş, `actor`).

Bu not M2'nin **nasıl** yapılacağını sabitler. M1'deki gibi: uygulama sırasında yanlış çıkan bir karar önce burada düzeltilir, sonra kodda.

## 1. Temel karar: belge ile dünya ayrıdır

```
 DİSK                        BELLEK
 main.scene.json  ⇄  SceneDocument (revizyon N, geri al yığını)   ← düzenleme: ajan, editör, dosya
                          │ instantiate (tek yön)
                          ▼
                       World  (oynatma oturumu: deterministik, diske yazmaz)
```

- **`SceneDocument`** sahnenin düzenlenebilir hâlidir: varlıklar **sahne kimliğiyle** (`"player"`), bileşenler **JSON değeriyle** tutulur. Tür bilgisi `wish_ecs::Registry`'den (şemalardan) gelir; belge bir `World` içermez.
- **Neden ECS dünyasında düzenleme yapmıyoruz?** Dünya çalışma zamanı içindir: varlık tutamaçları kararsızdır, değişiklik tick'leri ve olaylar vardır, sıra arketip düzenine bağlıdır. Belge ise insan ve ajanın gördüğü, diff'lenen, geri alınan şeydir. Kanonik biçim belgenin üzerinde tanımlıdır.
- **Dünyaya aktarım tek yönlüdür:** `scene.instantiate(&mut world) -> SceneMap` (sahne kimliği ⇄ `Entity`). Oynatma oturumundaki bir değeri belgeye geri almak açık bir işlemdir (M4'te `edit_apply` + oturumdan okuma).

## 2. Dosya formatı (`*.scene.json`, biçim 1)

```json
{
  "format": 1,
  "entities": {
    "camera": {
      "parent": "player",
      "components": {
        "Camera": { "fov_y_deg": 60 },
        "Transform": { "translation": [0, 2, 6] }
      }
    },
    "player": {
      "components": {
        "Transform": { "translation": [0, 1, 0] }
      },
      "notes": "Moved by the input system."
    }
  },
  "resources": {
    "Gravity": { "value": [0, -9.81, 0] }
  }
}
```

| Kural | Neden |
|---|---|
| Varlıklar **kimlik anahtarlı nesne**, dizi değil | Yamalar ve diff'ler indeksle değil kimlikle adreslenir (ADR-007) |
| Sahne kimliği: `[a-z0-9_]`, harfle başlar, ≤ 64 karakter | Dosya yollarında, JSON işaretçilerinde, komut satırında kaçışsız kullanılır |
| `parent` tek yapı kaynağıdır | Çocuk listesi ayrıca tutulmaz; iki doğruluk kaynağı eşitlenmez |
| Kardeş sırası: `order` (tamsayı, varsayılan 0), eşitlikte kimlik | Kimlik sırası çoğu sahnede yeterlidir; sıra önemli olduğunda `order` açıkça yazılır |
| Bileşen değerleri **yama** biçiminde: yalnızca varsayılandan farklı alanlar yazılır | Dosyalar kısa kalır; varsayılan değişirse sahne güncel varsayılanı alır |
| `notes` (isteğe bağlı metin) | JSON yorum desteklemez; ajan ve insan için açıklama alanı |
| `resources` bölümü | Sahneye özgü kaynak değerleri (yerçekimi vb.) |
| Hiyerarşi (`Hierarchy`) dosyada **yazılmaz** | Yönetilen bileşendir; `parent` + `order`'dan türetilir |
| Bilinmeyen üst düzey anahtar → tanı | "Sessizce yok sayılan alan" ajan için en kötü hata türüdür |

### Kanonik biçim (`wen fmt`)

- 2 boşluk girinti, LF satır sonu, dosya sonunda tek satır sonu, UTF-8 (BOM yok).
- Nesne anahtarları **bayt sırasıyla** (üst düzeyde de: `entities`, `format`, `resources`).
- Sayılar: tamsayılar ondalıksız; kesirli sayılar şemadaki türe göre (`f32` alanlar `f32`'nin en kısa gösterimiyle — M1'deki `to_json` kuralı).
- Boş nesneler `{}`; dizi öğeleri, hepsi sayıysa tek satırda (`[0, 1, 0]`), değilse alt alta.
- **Değişmez:** kanonik bir dosyayı yükleyip kaydetmek bayt düzeyinde aynı dosyayı verir. Kanonik olmayan dosya yüklenebilir; `wen fmt` onu kanonikleştirir, `wen check` uyarı verir.

Kendi JSON yazıcımızı yazıyoruz (`serde_json`'ın biçimlendiricisi dizi kuralını ve sayı kuralını desteklemiyor).

### Konumlu tanılar

`wen check` derleyici gibi **dosya + satır + sütun + JSON işaretçisi** verir. `serde_json` sözdizimi hatalarında satır/sütun verir ama anlamsal hatalarda (bilinmeyen alan, aralık dışı) vermez. **Karar:** küçük bir JSON ayrıştırıcısı yazıyoruz (`wish_scene::json`), her değerin kaynak aralığını (JSON işaretçisi → satır/sütun) kaydeder. M1'in `apply_json` tanıları zaten JSON işaretçisi taşıyor; işaretçi bu tablodan satır/sütuna çevrilir. Ayrıştırıcı katıdır (yinelenen anahtar → tanı; `serde_json` sessizce sonuncuyu alırdı).

## 3. Prefab'lar (`*.prefab.json`)

- Prefab, kökü `"root"` kimlikli bir sahne parçasıdır (aynı biçim, `resources` yok).
- Sahnede örnek: `{ "prefab": "prefabs/enemy.prefab.json", "components": { … } }`. `components` prefab kökünün **üzerine yama**dır.
- Prefab'ın kök dışı varlıkları örneğin altına `örnek_kimliği/prefab_kimliği` biçiminde açılır (`enemy_1/weapon`). Bu türetilmiş kimliklere sahnede yama yazılabilir: `"overrides": { "weapon": { "components": { … } } }`.
- **M2 kapsamı:** tek düzey prefab. İç içe prefab (prefab içinde prefab) M2'de tanı ile reddedilir; ihtiyaç M7'de (vitrin oyunu) ölçülür.
- Prefab döngüsü ve eksik prefab dosyası konumlu tanı üretir.

## 4. İşlemler, revizyonlar, geri al

```rust
pub enum EditOp {
    Spawn { id, parent: Option<SceneId>, components: Map<String, Value> },
    Despawn { id },                         // alt ağaçla birlikte
    Set { id, component, value },           // yama (M1 apply_json anlamı)
    Remove { id, component },
    Reparent { id, parent: Option<SceneId>, order: Option<i64> },
    Rename { id, to },                      // başvuran parent'lar da güncellenir
    Instantiate { id, prefab, parent },
    SetResource { name, value },
}
pub struct Transaction { pub actor: Actor, pub base_revision: Option<u64>, pub ops: Vec<EditOp> }
```

- **Atomik:** işlemin tamamı bir kopya üzerinde uygulanır ve doğrulanır; tek bir işlem bile reddedilirse belge değişmez, tanı `location.op` ile o işlemi gösterir.
- **İyimser eşzamanlılık:** `base_revision` verilip belge o arada değiştiyse `stale_revision` tanısı (güncel revizyonla).
- **Geri al:** her başarılı işlem, ters işlemlerden oluşan bir işlemle birlikte yığına girer. Geri al/yinele de birer işlemdir (revizyon artar). Yığın derinliği sınırlı (varsayılan 1000).
- **`actor`** (`Human` | `Agent(String)` | `File`): kimin yaptığı. Hibrit eşlik değişmezinin (ADR-018) temeli; etkinlik akışı M8b'de bunu gösterir.
- **Fark:** her işlem, değişen JSON işaretçilerinin listesini döndürür (M4'te `edit_apply` yanıtının `diff` alanı).
- Dosyaya doğrudan düzenleme (ADR-008): dosya izleyici dosyayı yeniden okur, yeni belgeyle eskisi arasındaki farkı **`actor: File`** işlemi olarak geçmişe kaydeder. Geçersiz dosya → tanı, bellekteki son geçerli belge korunur.

## 5. `wish_asset` (M2 kapsamı)

- **Varlık başvurusu** proje köküne göre yol + isteğe bağlı alt nesne: `"assets/models/crate.glb#Mesh0"`. M1 şema türlerine **`AssetRef`** türü eklenir (bileşende `Text` gibi tutamaç; dünyanın varlık tablosuna işaret eder). Şemada ayrı bir tür olduğu için JSON Schema ve doğrulama "bu bir dosya yolu" bilgisini taşır.
- **Varlık sunucusu (M2):** dosya keşfi, `*.meta.json` okuma/yazma, değişiklik izleme (`notify`), başvuru doğrulaması ("dosya yok" → konumlu tanı + benzer dosya önerisi). **İçe aktarma (glTF, doku) M3'tedir.**
- **`wen mv <eski> <yeni>`:** dosyayı taşır ve projedeki tüm sahne/prefab başvurularını aynı işlemde günceller.
- **Dosya izleme determinizmi:** izleyici olayları simülasyona doğrudan girmez; belge düzeyinde işlem olarak sıraya girer.

## 6. `SceneDocument` → `World`

- Her sahne varlığı için bir `Entity`; tür adları kayıttan çözülür, değerler M1 `set_json` ile yazılır. Bilinmeyen bileşen adı → belge yüklenirken zaten tanı.
- Hiyerarşi `parent` + `order`'dan kurulur (`set_parent`, kardeş sırasıyla).
- **Sahne kimliği dünyada:** yönetilen bir `SceneId { id: Text }` bileşeni. Ajan oynatma oturumunda da `"player"` diye adresleyebilir; `entity_query(name)` bunu kullanır.
- **Aktarım deterministiktir:** varlıklar ebeveyn önce, kardeşler sırayla, kökler kimlik sırasıyla oluşturulur. Aynı belge → aynı dünya hash'i (test).

## 7. Komut satırı

| Komut | Ne yapar |
|---|---|
| `wen check [yollar…] [--json]` | Proje (ya da verilen dosyalar): sözdizimi, şema, başvurular, kanonik biçim. Tanılar dosya/satır/sütun/işaretçi ile |
| `wen fmt [yollar…] [--check]` | Kanonikleştirir; `--check` yalnızca bildirir (CI için) |
| `wen mv <eski> <yeni>` | Varlık/sahne dosyasını taşır, başvuruları günceller |
| `wen migrate [yollar…]` | Eski `format` sürümlerini yükseltir (M2'de yalnızca altyapı; biçim 1 tek sürüm) |
| `wen run <sahne>` | Sahneyi yükler, sistemsiz çalıştırır (sistemler M5'te Wasm'dan gelecek); hash'i raporlar |

Hepsi M1'deki gibi `--json` ile MCP zarfında çıktı verir.

## 8. M1'den devralınanlar

- Derleme hatası testleri (`trybuild`) ve makronun düz veri olmayan alan için yinelenen hata iletileri.
- `syn` 3'e geçiş (makro).
- ~~CI'ın ilk gerçek çalıştırması~~ — 15 Kasım 2026'da yapıldı: özel GitHub deposu açıldı, CI Windows ve Linux'ta yeşil.

## 9. Test planı (çıkış ölçütleriyle)

| Çıkış ölçütü | Test |
|---|---|
| Yükle → kaydet bayt düzeyinde aynı | Örnek sahneler + özellik testi: rastgele belge → kanonik yaz → oku → yaz = aynı bayt |
| 30 bozuk dosyanın her biri konumlu, ipuçlu tanı | `tests/broken/*.scene.json` + beklenen tanı (kod, satır, sütun, işaretçi) altın dosyaları |
| Dosya değişince dünya < 1 sn güncellenir; geçersiz dosya son geçerli durumu bozmaz | İzleyici testi (geçici klasör) |
| 100 rastgele işlem + 100 geri al = başlangıç dosyası | Özellik testi (model tabanlı) |
| Aynı belge → aynı dünya hash'i | Aktarım testi |

## 10. İş paketleri ve önerilen model

| # | İş paketi | Model · efor |
|---|---|---|
| WP1 | `wish_scene::json`: aralık kaydeden katı ayrıştırıcı + kanonik yazıcı | Sonnet 5.5 · high — **tamamlandı** |
| WP2 | `SceneDocument`: yükleme, doğrulama (şema, kimlik, parent, döngü), konumlu tanılar, 30 bozuk dosya testi | Sonnet 5.5 · high — **tamamlandı** |
| WP3 | İşlemler, revizyonlar, geri al/yinele, `actor`, fark; model tabanlı test | Opus 5.5 · high |
| WP4 | Prefab'lar (tek düzey, yamalar, türetilmiş kimlikler) | Opus 5.5 · high |
| WP5 | `SceneId`, belge → dünya aktarımı, `wen run <sahne>` | Sonnet 5.5 · medium |
| WP6 | `wish_asset`: `AssetRef` şema türü, keşif, `*.meta.json`, izleme, `wen mv` | Sonnet 5.5 · high |
| WP7 | `wen check / fmt / migrate`, dosya izleyici ile sıcak yükleme | Sonnet 5.5 · medium |
| WP8 | M1 devirleri (`trybuild`, `syn` 3), kapanış incelemesi | Opus 5.5 · high |

## 11. Açık sorular

1. **`order` mı çocuk listesi mi?** `parent` + `order` seçildi (tek kaynak). Editörde sürükle-bırak ile sıralama çok sayıda `order` yeniden numaralandırması gerektirirse M8a'da yeniden değerlendirilir.
2. **Varsayılandan farklı alanları yazma kuralı** varsayılan değiştiğinde sahnenin anlamını sessizce değiştirir. Bu bilinçli bir seçim (prefab'larda istenen davranış). İstenmezse `wen fmt --explicit` tüm alanları yazar.
3. **Sayı biçimi:** bileşen değerleri şemaya göre biçimlenir, ama `notes` dışındaki serbest JSON yok; şemasız bir sayı ortaya çıkarsa (ör. bilinmeyen bileşen) tanı verildiği için biçim sorunu doğmaz.

## 12. Uygulama notları

### WP1 — `wish_scene::json`

- **Ağaç:** `parse(kaynak, dosya) -> Node`. Her `Node` değerinin başladığı `Span`'ı (satır, sütun; 1'den, **karakter** olarak) taşır; nesne üyeleri ayrıca anahtarın konumunu taşır. `Node::locate(işaretçi)` bir JSON işaretçisini (M1'in şema tanıları bunu taşıyor) `Located { key, value }`'ya çevirir: bilinmeyen alan anahtara, aralık dışı sayı değere işaret edebilsin diye ikisi ayrı.
- **Katılık:** yinelenen anahtar `json_duplicate_key`; sondaki virgül, yorum, tek tırnak, baştaki sıfır, `NaN`/`Infinity`, ham denetim karakteri, geçersiz kaçış, tek başına vekil (surrogate) yarısı ve dosya sonundaki fazlalık ayrı ayrı, konumlu ve ipucu taşıyan tanılardır. BOM okunur ama dosya kanonik sayılmaz. İç içe geçme sınırı 128 (hem okumada hem yazmada).
- **Kanonik yazıcı:** `to_canonical(&Value) -> Result<String>`. Anahtarlar **açıkça** UTF-8 bayt sırasıyla sıralanır (`serde_json::Map` sırası bir özelliğe bağlı olduğu için ona güvenilmedi). Yalnızca sayılardan oluşan diziler tek satırda; boş `[]`/`{}`; dizelerde yalnızca `"`, `\`, denetim karakterleri kaçışlanır. Sayılar `serde_json::Number`'ın en kısa gösterimiyle (`1e2` → `100.0`, `0.10` → `0.1`). `is_canonical(kaynak)`: bayt düzeyinde eşitlik.
- **Doğrulama:** rastgele `Value` → kanonik → oku → aynı değer ve aynı bayt (sabit nokta); her konum gerçekten o değerin ilk karakterine işaret ediyor; rastgele metinde ayrıştırıcı çökmüyor, hata her zaman konum ve ipucu taşıyor.
- **Testin yakaladığı hata:** yüksek vekil yarısı geçersiz bir ikinci yarıyla gelince birleştirme hesabı taşıyordu (kötü niyetli girdide çökme). Hesap, aralık doğrulandıktan sonraya alındı; iki örnek test olarak eklendi.
- **Hata ipuçları kendi başına yeterli olmalı:** testler ipucunun ilgili terimi (ör. "surrogate", "four") içermesini zorluyor; ileti ile ipucu birbirinin yerini tutmaz.

### WP2 — `SceneDocument` ve doğrulama

- **`load(kaynak, dosya, &Registry) -> LoadResult { document, diagnostics }`.** Sözdizimi hatasında ilk hatada durur (ayrıştırıcı katıdır); sözdizimi geçtikten sonra **tüm** anlamsal sorunları toplar ve dosya sırasıyla verir: `wen check` tek çalıştırmada her şeyi gösterebilsin diye. Belge yalnızca hata yoksa döner; uyarılar yüklemeyi engellemez.
- **Her tanı dosya + satır + sütun + JSON işaretçisi taşır.** Şema tanıları (M1'in `from_json`) kendi işaretçisini verir; `Node::locate` onu konuma çevirir. `unknown_field` anahtarın, diğerleri (aralık, tür, uzunluk, bilinmeyen varyant) değerin konumuna işaret eder.
- **Yeni tanı kodları:** `missing_format`, `unsupported_format`, `unknown_key` (üst düzey ya da varlık anahtarı; ""şunu mu demek istediniz"" ile), `invalid_scene_id`, `unknown_parent`, `hierarchy_cycle` (döngü bir kez, en küçük kimliğin `parent` değerinde; ileti döngüyü `a -> b -> c -> a` diye yazar), `unknown_component` (adaylar yönetilen bileşenleri içermez), `managed_component` (`Hierarchy` dosyada yazılamaz), `unknown_resource`, `not_canonical` (uyarı). Mevcut `type_mismatch` ve şema kodları aynen kullanılıyor.
- **Bileşen değerleri JSON olarak, dosyadaki hâliyle saklanır** (yama biçimi; varsayılanlar eklenmez). Doğrulama için bir kopya `from_json` ile kontrol edilir; metinler atılan bir tabloya işlenir.
- **Kanonik biçim belge düzeyinde de sabit nokta:** `to_canonical` boş bölümleri yazmaz (`entities`, `resources`, boş `components`, 0 olan `order`). `not_canonical` uyarısı "kaynak = belgeden üretilen kanonik metin" karşılaştırmasıdır; boş bölümleri açıkça yazan dosya da uyarı alır. Testler: gevşek dosya → uyarı + belge; yazılan kanonik metin → sıfır tanı ve aynı bayt.
- **Test:** 35 bozuk dosya (her biri tek bir yanlış; beklenen kod, satır, sütun ve ipucundaki anahtar kelime); döngü + 3 bağımsız hata aynı dosyada beş yerine dört tanı verir; işaretçiler; dosya yolu olmayan yükleme.
- **Bilinçli sınırlar:** `prefab`/`overrides` anahtarları WP4'e kadar `unknown_key` verir; sahne kimlikleri yalnızca ASCII (`[a-z][a-z0-9_]*`, ≤ 64).