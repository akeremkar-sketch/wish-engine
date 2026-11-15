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
- **Bilinçli sınırlar:** iç içe prefab yok (§3: ihtiyaç M7'de ölçülür), dolayısıyla prefab döngüsü de oluşamaz. Sahnedeki bir varlık türetilmiş bir varlığın (`enemy_1/weapon`) altına bağlanamaz; yalnızca örneğin kendisine bağlanabilir. Örneği "açıp" sıradan varlıklara çevirme (unpack) yok. Bir prefab belgesine işlemlerle dokunulursa prefab kuralları ancak dosya yeniden yüklenince denetlenir (prefab düzenleme modu M8a'da).