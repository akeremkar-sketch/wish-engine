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
| WP3 | İşlemler, revizyonlar, geri al/yinele, `actor`, fark; model tabanlı test | Opus 5.5 · high — **tamamlandı** |
| WP4 | Prefab'lar (tek düzey, yamalar, türetilmiş kimlikler) | Opus 5.5 · high — **tamamlandı** |
| WP5 | `SceneId`, belge → dünya aktarımı, `wen run <sahne>` | Sonnet 5.5 · medium — **tamamlandı** |
| WP6 | `wish_asset`: `AssetRef` şema türü, keşif, `*.meta.json`, `wen mv` (izleme WP7'ye) | Sonnet 5.5 · high — **tamamlandı** |
| WP7 | `wen check / fmt / migrate`, dosya izleyici ile sıcak yükleme | Sonnet 5.5 · medium — **tamamlandı** |
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

### WP3 — işlemler, geri al, etkinlik günlüğü

- **İşlemler (`EditOp`):** `Spawn` (ebeveyn, sıra, bileşenler, not), `Despawn` (alt ağaçla), `Set` (birleştirme: adı geçen alanlar değişir), `Replace` (tam değer), `Remove`, `Reparent` (sıra isteğe bağlı), `Rename` (çocukların `parent`'ı da güncellenir), `SetNotes`, `SetResource` / `ReplaceResource` / `RemoveResource`. §4 taslağına göre eklenenler: `Replace` ve `ReplaceResource` (geri almanın kesin tersi için gerekli; ajanlar da kullanabilir), `SetNotes`. `Instantiate` (prefab) WP4'te.
- **`Set` birleştirmesi** M1'in `apply_json` anlamıyla aynıdır: nesneler anahtar anahtar birleşir, diziler ve diğer değerler tümüyle değişir.
- **Hep ya da hiç:** işlem belgenin bir kopyasına sırayla uygulanır; herhangi bir adım reddedilirse orijinal belgeye dokunulmaz, tanı `location.op` ile adımı, `location.path` ile JSON işaretçisini gösterir. Bileşen ve kaynak değerleri yüklemedeki şema denetiminin aynısından geçer (ortak `schema_check` modülü: aynı hata aynı tanıyı alır).
- **Ters işlemler uygulama anında hesaplanır** ve geri alma bunları tersten uygular. Silinen alt ağaç ebeveyn önce olmak üzere tam hâliyle (bileşenler, notlar, sıra) geri gelir; yeniden adlandırma çocukları da geri taşır.
- **Revizyonlar:** her başarılı işlem, geri alma ve yineleme revizyonu 1 artırır. `base_revision` uyuşmazsa `stale_revision`; boş işlem `empty_transaction`. `preview` hiçbir şeyi değiştirmeden fark listesini verir (M4'te `dry_run`).
- **Fark listesi:** her işlem değişen JSON işaretçilerini sıralı döndürür.
- **Kim yaptı:** `Actor::Human` / `Agent(ad)` / `File`. Etkinlik günlüğü (`log()`, en çok 1000) her düzenleme, geri alma ve yinelemeyi revizyon, aktör ve değişikliklerle kaydeder: hibrit eşlik değişmezinin (tek geçmiş, `actor`) belge düzeyindeki karşılığı. Geri alma yığını en çok 1000 işlem; yeni bir düzenleme yineleme yığınını temizler.
- **Belge eşitliği** yalnızca içeriği karşılaştırır (varlıklar ve kaynaklar), revizyon ve geçmişi değil.
- **Yeni tanı kodları:** `unknown_entity`, `entity_exists`, `component_not_found`, `resource_not_found`, `stale_revision`, `empty_transaction`.
- **M2 çıkış ölçütü karşılandı (geri al):** model tabanlı test, 128 senaryoda 100'e kadar rastgele işlem (bir kısmı bilerek geçersiz: aralık dışı değer, döngü, olmayan bileşen) uygular. Her başarılı adımdan sonra belge kanonik metninden **sıfır hatayla** yeniden yüklenir; her reddedilen adım belgeyi bayt düzeyinde aynı bırakır. Sonra hepsi geri alınır: başlangıç dosyası bayt bayt geri gelir. Hepsi yinelenir: son dosya bayt bayt geri gelir.

### WP4 — prefab'lar

- **Prefab kütüphanesi:** `PrefabLibrary` (yol → prefab belgesi, sıralı). `load_prefab(kaynak, dosya, &Registry)` sahne doğrulayıcısını "prefab" kipinde çalıştırır; ek kurallar aynı geçişte, konumlu: `prefab_without_root`, `prefab_root_with_parent`, `prefab_entity_outside_root` (kök dışındaki her varlığın bir ebeveyni olmalı, yani her şey kökün altında), `prefab_with_resources`, `nested_prefab` (tek düzey; §3).
- **Sahne belgesi kütüphaneyi `Arc` ile paylaşır** (`load_with_prefabs`, `set_prefabs`, `prefabs()`); eşitliğe girmez. `load` boş kütüphaneyle çalışır: örnek içeren sahne `prefab_not_found` ("No prefabs are loaded") verir. Prefab dosyalarının keşfi WP6'da (`wish_asset`), değişen prefab'ın sıcak yüklenmesi WP7'de.
- **Örnek:** `prefab` anahtarı olan varlık. `components` prefab kökünün bileşenlerine, `overrides.<prefab_varlığı>.components` diğer prefab varlıklarınınkine yamadır. Yamalar tek başına şemaya göre doğrulanır: birleştirme (M1'in `apply_json` anlamı) geçerli iki yamadan geçerli bir yama üretir, ayrıca birleşik değeri denetlemeye gerek yok.
- **Yeni tanı kodları (yükleme):** `prefab_not_found` (yol için "şunu mu demek istediniz"), `overrides_without_prefab`, `override_on_root` (kök değişiklikleri örneğin kendi `components`'ına yazılır: tek yer), `unknown_prefab_entity`. Override içinde `components` dışındaki anahtar `unknown_key`.
- **Karar — bilinmeyen override hedefi hatadır, uyarı değil.** Prefab'dan bir varlık silinince ya da adı değişince ona yazılmış override'lar sessizce yok sayılsaydı veri gizlice kaybolurdu (ve geri al, artık geçersiz bir hedefi geri yazamazdı). Bunun yerine sahne yüklenirken hangi override'ın neden geçersiz olduğu konumuyla söylenir; ipucu "yeniden adlandır ya da sil" der. Prefab değişince bağımlı sahnelerin yeniden doğrulanması ve otomatik yeniden adlandırma WP7'nin (sıcak yükleme) işi.
- **Açma:** `SceneDocument::expand() -> Vec<ExpandedEntity { id, parent, order, components }>`. Örnek X prefab kökü olur (bileşenler: kök ⊕ örnek yaması); prefab varlığı p → `X/p`, ebeveyni `root` ise X, değilse `X/<ebeveyn>`; bileşenler p ⊕ `overrides[p]`. Sahne kimlikleri `/` içeremediği için türetilmiş kimlikler sahnedekilerle çakışamaz. Sıra kararlı: ebeveyn önce, kardeşler `(order, kimlik)` sırasıyla, örneğin prefab varlıkları örneğin hemen ardından, sonra örneğin sahnedeki çocukları. WP5 (belge → dünya) bunu kullanacak.
- **İşlemler:** `Instantiate` (kimlik, prefab, ebeveyn, sıra, kök yamaları, override'lar, not), `SetOverride` (birleştirme), `ReplaceOverride`, `RemoveOverride` (boşalan override girdisi dosyadan da kalkar). Örneğin silinmesinin tersi tam durumla `Instantiate`'tir. Örnek üzerindeki `Set/Replace/Remove` yalnızca örneğin yamasını değiştirir: `Remove` prefab'ın değerini geri getirir. Yeni kodlar: `not_an_instance`, `override_not_found`.
- **Test (`tests/prefabs.rs`, 10 test):** açma ve türetilmiş kimlikler; kanonik gidiş-dönüş (sıfır tanı, aynı bayt); 5 prefab kuralı + 8 örnek hatası (kod, satır, sütun, ipucundaki anahtar kelime); işlemler + geri al/yinele; 8 reddedilen işlem (doğru kod, `location.op`, belge aynı); model tabanlı özellik testi (96 senaryo, 60'a kadar rastgele örnekleme/override/silme/yeniden adlandırma adımı, bir kısmı bilerek geçersiz): her başarılı adımdan sonra sıfır tanıyla yeniden yüklenir ve açılır, hepsini geri almak başlangıç dosyasını, yinelemek son dosyayı bayt bayt verir.
- **Bilinçli sınırlar (WP4):** iç içe prefab yok (§3: ihtiyaç M7'de ölçülür), dolayısıyla prefab döngüsü de oluşamaz. Sahnedeki bir varlık türetilmiş bir varlığın (`enemy_1/weapon`) altına bağlanamaz; yalnızca örneğin kendisine bağlanabilir. Örneği "açıp" sıradan varlıklara çevirme (unpack) yok. Bir prefab belgesine işlemlerle dokunulursa prefab kuralları ancak dosya yeniden yüklenince denetlenir (prefab düzenleme modu M8a'da).

### WP5 — belge → dünya, `SceneId`, `wen run --scene`

- **`SceneDocument::instantiate(&mut World) -> SceneEntities`** (`wish_scene::instantiate`): `expand()` çıktısının her varlığı için bir `Entity`, `SceneId { id: Text }`, bileşenler `set_json` ile, sonra hiyerarşi (`World::set_parent`, ebeveyn önce ve kardeşler `order` sırasıyla eklendiği için kardeş sırası korunur), sonra sahne kaynakları. Dönen harita sahne kimliğinden (türetilmiş `enemy_1/weapon` dahil) `Entity`'ye. Aktarım işlemsel değildir (reddedilen bir değerde dünya kısmen dolu kalabilir); aynı kayıtla `load` edilmiş belge burada hata vermemelidir.
- **Karar — `SceneId` yönetilen (`managed`) değil, ayrılmış bir bileşen.** Tasarım §6 yönetilen demişti; ama yönetilen işareti `wish_ecs` içinde (unsafe'li crate, Miri) yeni bir yazma/kayıt yolu gerektirirdi. Bunun yerine `wish_scene` onu sıradan bileşen olarak kaydeder (`instantiate::register`) ve `schema_check::component` sahne dosyasında yazılmasını `managed_component` koduyla reddeder; aday listesinden de çıkarılır. Sonuç kullanıcıya aynı: dosyada yazılamaz, her varlık anahtarından alır. `wish_ecs`'e dokunulmadı.
- **`wen run --example <ad> --scene <dosya> [--root <klasör>]`:** `--scene` ile örneğin kendi varlıkları ve sistemleri yüklenmez; örnek yalnızca **bileşen/kaynak türlerini** sağlar (`wish_examples::register_types`), çünkü oyun modülleri (Wasm) M5'e kadar yok. `--root` (varsayılan geçerli klasör) altındaki tüm `*.prefab.json` dosyaları, yola göre sıralı, `/` ayraçlı proje-göreli yollarla kütüphaneye yüklenir; sahne yolu da buna göredir. Çıktıya `scene`, `scene_entities` ve `warnings` (örn. `not_canonical`) eklenir.
- **Hata davranışı:** sahne ya da prefab dosyasında hata varsa `wen run` ilk hatayı (dosya, satır, sütun, ipucu) döner; ipucuna "N başka sorun var" notu eklenir (hepsini tek seferde görmek `wen check` ile WP7'de). Okunamayan dosya: `file_unreadable`.
- **Test:** `tests/instantiate.rs` (3): kimlikler/hiyerarşi/kardeş sırası/override/kaynak değerleri, aynı belge → aynı dünya hash'i (boş sahneden farklı), `SceneId` dosyada reddi. `wish_cli/tests/wen.rs` (+2): geçici proje klasöründe sahne + prefab çalıştırma (iki çalıştırma aynı hash), konumlu hata ve eksik dosya.
- **Bilinçli sınır:** bir örneğin prefab çocukları ile sahnedeki çocukları aynı ebeveyn altında `order`'la karıştırılmaz; önce prefab varlıkları, sonra sahne çocukları gelir.

### WP6 — `AssetRef` ve `wish_asset`

- **`AssetRef` bir şema türüdür** (`TypeKind::AssetRef`, parmak izi etiketi 23; mevcut etiketler değişmedi, kayıtlı hash'ler tuttu). Depolama `Text` ile aynıdır (metin tablosuna 4 baytlık tutamaç; `wish_core::AssetRef` `Text` üzerinde `repr(transparent)`, boş yol = "varlık yok"), ama JSON'a girerken yol biçimi denetlenir (`invalid_asset_ref`, ipucuyla): boş değilse göreli, `/` ayraçlı, `.`/`..`/boş parça yok, ters eğik çizgi, sürücü harfi ve denetim karakteri yok, en çok bir `#alt_nesne`, ≤ 512 bayt. JSON Schema çıktısı `"x-asset": true` taşır, böylece araçlar ve ajanlar dosya seçici/doğrulama sunabilir. `contains_text` ve dünya metin sayımı `AssetRef`'i de kapsar (aynı tablo).
- **Karar — izleme WP7'ye:** `notify` yeni bir dış bağımlılık; zaten sıcak yüklemeyle (WP7) birlikte kullanılacağı için orada eklenip lisansı denetlenecek. WP6 kapsamı: keşif, meta, başvuru denetimi, `wen mv`.
- **`wish_asset`:** `wish_core`, `wish_ecs`, `wish_scene`, `wish_schema`'ya dayanır (sahne belgeleri üstündeki katman; `wish_scene` varlık dosyalarını bilmez).
  - `AssetIndex::scan(kök)`: gizli klasörler (`.` ile başlayan) ve motor belgeleri (`*.scene.json`, `*.prefab.json`, `*.meta.json`) dışındaki tüm dosyalar, sıralı. `check(başvuru)`: biçim → `invalid_asset_ref`, dosya yok → `asset_not_found` ("şunu mu demek istediniz" ya da boş-proje ipucu). `#alt_nesne` denetlenmez: içeriği ancak içe aktarmadan (M3) sonra bilinir.
  - `*.meta.json`: `{ "format": 1, "import": { … } }`, konumlu ve ipuçlu ayrıştırma (`parse_meta`), kanonik yazma, `read_meta`/`write_meta`. `import` içeriğini içe aktarıcılar tanımlayacak (M3); şimdilik olduğu gibi tutulur. Meta dosyası olmayan varlık varsayılan ayarları kullanır.
  - Başvuru bulma **şemayla** yapılır, dize benzerliğiyle değil: sahne/prefab JSON'unda varlık bileşenleri, örneklerin `overrides` bileşenleri ve `resources` bileşen şemasına göre gezilir (yapı alanları, diziler). `check_source` her kötü başvuruyu dosya/satır/sütun ve JSON işaretçisiyle verir; sözdizimi hatası tek tanı olarak döner.
  - **`wen mv <eski> <yeni> --example <ad> [--root] [--dry-run]`:** dosya ya da klasör taşır; tüm sahne/prefab belgelerindeki başvuruları (`#alt_nesne` korunur, klasör önekleri eşlenir, benzer önekli klasör etkilenmez) aynı işlemde günceller; dosyanın `.meta.json`'u onunla gider. Reddedilenler: olmayan kaynak (`asset_not_found`, öneriyle), var olan hedef (`asset_exists`, hiçbir şey ezilmez), klasörü kendi içine taşıma, aynı yol, geçersiz yol, sözdizimi hatalı bir belge (plan aşamasında durur, hiçbir şey taşınmaz). **Hep ya da hiç:** önce plan (diske dokunmaz), sonra uygulama; bir adım başarısız olursa yapılanlar geri alınır (salt okunur sahneyle test edildi). Değişen belgeler kanonik biçimde yeniden yazılır (anlam değişmez; gevşek biçimli dosya da düzelir).
  - `--example` gerekli çünkü bileşen türleri (hangi alanın `AssetRef` olduğu) oradan gelir; oyun modülleri (M5) gelince proje kendi türlerini getirecek. `stress` örneğine yalnızca sahne için kayıtlı `Model { mesh, textures }` bileşeni eklendi (kıyaslama dünyasına/hash'lerine girmez).
- **Miri:** WP6'da `wish_ecs`/`Plain` bildirimine dokunuldu; `cargo +nightly miri test -p wish_ecs` temiz.
- **Hibrit eşlik:** `wen mv` dosya düzeyinde bir insan işlemidir (sahne belgesi açık değilken); açık belgelerin işlem geçmişine (`actor`) bağlanması düzenleyiciyle (M8a) ve sıcak yüklemeyle (WP7) gelir.
- **Test:** `wish_asset/tests/assets.rs` (9): şema ve biçim denetimi (`set_json` reddi, `x-asset`), dizin, meta (gidiş-dönüş + 5 hata konumu), şemaya göre başvuru bulma, konumlu eksik dosya, dosya taşıma (+ meta, + taşımadan sonra tüm başvurular çözülür), klasör taşıma, 6 reddedilen taşıma, geri alma. `wen mv` CLI testi (+1). `wish_core::asset` birim testleri (3).

### WP7 — `wen check / fmt / migrate / watch`, sıcak yükleme

- **Karar — izleyici yoklama (polling) ile, `notify` olmadan.** `Watcher::poll()` proje klasörünü (gizli klasörler hariç) değişiklik zamanı + boyutla karşılaştırır ve eklenen/değişen/silinen yolları verir. Gerekçe: sıfır bağımlılık (lisans/indirme yok), her platformda aynı davranış, proje boyutunda ucuz; gerekirse yerel bildirim arka ucu aynı `poll()` arayüzünün arkasına konur. Sınır: aynı boyutta ve aynı zaman damgalı bir değişiklik kaçar (dosya sistemi zaman çözünürlüğü). Dosya sistemi saatini okur, simülasyon saatine dokunmaz.
- **`load_project(kök, &Registry)` (`wish_asset::project`):** tüm prefab'lar (önce), sahneler ve meta dosyaları yüklenir ve birlikte denetlenir; prefab kuralları, sahne doğrulaması, varlık dosyalarının varlığı ve meta biçimi tek geçişte; tüm tanılar yol sırasıyla, geçerli belgeler ve her dosyanın kanonik metni (hatalıysa `None`) döner. `wen check` ve `wen fmt` bunun üstündedir.
- **`wen check [dosyalar…] --example <ad> [--root] [--json]`:** tüm sorunları dosya/satır/sütun/işaretçi ve ipucuyla verir; yalnızca hata varsa başarısız çıkış (uyarı, örn. `not_canonical`, başarısız saymaz). Çıktı `{ok, files, errors, warnings, diagnostics}`. Dosya adı verilirse yalnızca onların tanıları raporlanır (öbürleri yine yüklenir: prefab'lar gerekli).
- **`wen fmt [dosyalar…] [--check]`:** belgeyi yükleyip `to_canonical` ile yazar (boş bölümler atılır, anahtarlar sıralanır). Hatalı dosyalara dokunmaz, `skipped` ile raporlar ve başarısız çıkar. `--check` hiçbir şeyi değiştirmez, kanonik olmayanları listeler ve onlar varsa başarısız çıkar (CI için).
- **`wen migrate`:** biçim 1 tek sürüm olduğundan altyapıdır: güncel dosyaları sayar, daha yenisi için `unsupported_format` ("motoru güncelle"), daha eskisi için `no_migration`, `format` yoksa `missing_format`. Biçim 2 gelince adımlar buraya eklenir.
- **Sıcak yükleme `HotScene` (`wish_asset::hot`):** `start` projeyi yükler (geçerli değilse ilk hatayla durur: geri dönülecek bir önceki iyi durum yok) ve sahneyi dünyaya aktarır; `poll(&mut World)` dosya değiştiyse projeyi yeniden yükler. **Proje geçerliyse** sahnenin varlıkları yok edilip yenisi kurulur (`applied: true`); **geçersizse** dünyaya dokunulmaz, son iyi sahne çalışmaya devam eder ve sorunlar `Reload.diagnostics` ile döner (`applied: false`). Eksik varlık dosyası da yeniden yüklemeyi engeller. Sahne dışı varlıklara dokunulmaz. Dünya yalnızca `poll` içinde, tick'ler arasında değişir.
- **Bilinçli sınır — sahne bütün olarak değiştirilir.** Çalışırken sahne varlıklarının kazandığı durum kaybolur; varlık başına fark uygulaması (kimlik eşlemesiyle yalnızca değişen bileşenleri yazmak) M3/M8a'da ölçülüp eklenir. Yeniden yüklenen dünyanın içeriği taze yüklemeyle aynıdır ama **dünya hash'i aynı değildir**: yenilenen varlıklar yuvalarının kuşak sayısını artırır (eski tutamaçlar yeni varlıkları gösteremesin diye); bu doğru davranıştır, test içeriği varlık başına karşılaştırır.
- **`wen watch --example <ad> --scene <dosya> [--root] [--seconds n] [--json]`:** sahneyi yükler ve 100 ms'de bir yoklar; `--json` ile her olay bir JSON satırıdır (`loaded`, `reloaded`, `rejected`, sonunda `{ok, reloads, rejected}`). Bu, sıcak yüklemenin kontrol düzleminden gözlemlenebilir yüzeyidir (MCP'de olay akışı M4'te). Güncelleme gecikmesi en çok bir yoklama aralığıdır (çıkış ölçütü: < 1 sn).
- **Çıkış durumu:** `check`/`fmt`/`migrate` sorun bulunca kendi çıktılarını (`ok: false` + `diagnostics`) basar ve başarısız çıkar; diğer komutlar tek `{ok:false,error}` tanısı verir.
- **Test:** `wish_asset/tests/project.rs` (4): çok dosyalı çok hatalı proje, izleyici, hot reload (geçerli → uygulanır, hatalı → reddedilir ve hash aynı, eksik varlık, düzeltince içerik taze yüklemeyle eşit), başlangıç hataları. `wish_cli/tests/project.rs` (4): `check`, `fmt` (+ `--check`), `migrate`, `watch` (ilk `loaded` satırı beklenip düzenlenir: `loaded → reloaded → rejected`).