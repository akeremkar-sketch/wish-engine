# M3 — İlk piksel: tasarım notu

Durum: yazıldı (15 Kasım 2026). Kod yazmadan önce oku. Bağlam: [ROADMAP](../ROADMAP.md) M3, [DECISIONS](../DECISIONS.md) ADR-012/013, [ARCHITECTURE](../ARCHITECTURE.md) "Render hattı".

## 1. Hedef ve sınırlar

M3 sonunda: bir sahne dosyası (M2) **pencere açmadan** PNG'ye çizilir; modeller glTF'den gelir; "şu pikselde hangi varlık var" sorusu kimlik tamponundan yanıtlanır; gölgelendirici hataları ortak tanı formatında dosya/satırla gelir. Pencere en sonda, ince.

**Kapsam dışı (M3'te yapılmaz, M10 listesine):** animasyon/iskelet (M6), kümelenmiş ışık (tek yönlü ışık yetiyor; nokta/spot ışıklar M6'da), IBL/ortam haritası (M3'te prosedürel gökyüzü + sabit ortam ışığı), MSAA, ardıl işlemler (ton eşleme hariç), bindless/GPU güdümlü çizim. Bunlar "taban çizgisi" (ADR-012) içinde ama sonraki kilometre taşlarında; render grafiği (§4) bunları sonradan eklemeyi mümkün kılar.

**Değişmezler:** 2 (determinizm): render determinizm sınırının **dışındadır** (ADR, mimari §Kare döngüsü) — simülasyon durumu GPU'dan hiç okunmaz, render ECS'e yazmaz. 8 (gözlemlenebilirlik): her render özelliği `wen capture`/kimlik sorgusuyla görülebilir ve başsız testle sınanır. 5 (tanı): gölgelendirici hatası da `Diagnostic`.

## 2. Crate'ler ve bağımlılık yönü

| Crate | Sorumluluk | Dış bağımlılıklar |
|---|---|---|
| `wish_import` | glTF/GLB → CPU verisi (`Model`: ağlar, materyaller, dokular, düğüm ağacı). GPU bilmez | `gltf`, `image` |
| `wish_transform` | `Transform` (yerel), `GlobalTransform` (türetilmiş), hiyerarşi boyunca yayılım sistemi | — |
| `wish_render` | wgpu aygıtı, kaynaklar, render grafiği, PBR, gölge, gökyüzü, ton eşleme, hata ayıklama görünümleri, yakalama, ECS'ten sahne çıkarımı | `wgpu`, `naga`, `pollster`, `bytemuck`, `png` |
| `wish_window` | `winit` penceresi, sunum yüzeyi, basit uçuş kamerası (en son iş paketi) | `winit` |

Yön: `wish_render` → `wish_import` (veri türleri), `wish_transform`, `wish_ecs`, `wish_asset` (varlık yolu çözme); `wish_window` → `wish_render`. `wish_import` ve `wish_transform` render'ı tanımaz. wgpu yalnızca `wish_render`'da (değişmez 6: dış kütüphane yalnızca kendi sarmalayıcısında). `wen` (CLI) geçici ev sahibidir; kontrol düzlemi `wish_control` M4'te gelince `capture`/`describe_view` oraya taşınır, işlev `wish_render` API'sinde kalır (değişmez 4: önce kontrol düzlemi — M4'e kadar `wish_render` işlevleri tek giriş noktasıdır, CLI sadece çağırır).

## 3. Bileşenler (şema önce)

Hepsi `#[component]`, belge yorumlu, birimli:

- `Transform { translation: Vec3 (m), rotation: Quat, scale: Vec3 }` — sahne dosyasında `translation`/`rotation`/`scale`; `rotation` için ek olarak `euler_deg` kısa yazımı **M3'te yok** (mimari belgedeki örnek gösterim amaçlıydı; kesin biçim: dörtlü). Gerekirse M4'te editör/ajan kolaylığı olarak sorulur.
- `GlobalTransform` — yazılamaz (motor türetir, `Hierarchy` gibi). **Karar:** WP2'de `wish_ecs`'e yeni yönetilen-bileşen kaydı eklemek yerine (unsafe'li crate), M2'deki `SceneId` yolu izlenir: sahnede yazılması `managed_component` ile reddedilir.
- `Camera { fov_y_deg (1–179), near (m), far (m) }` — `aspect` yakalama boyutundan gelir. Sahnede birden çok kamera olabilir; yakalama `--camera <sahne_kimliği>` ile seçer (yoksa ilk kamera).
- `DirectionalLight { color: Color, illuminance_lux, shadows: bool }` — yön, varlığın `GlobalTransform`'unun −Z ekseni.
- `Model { asset: AssetRef }` — bir glTF/GLB'nin sahnesini bu varlığın altına açar. **Karar (M2 prefab'ıyla ilişki):** glTF düğümleri sahne varlıkları **olmaz**; `Model` tek varlıktır ve içindeki düğüm ağacı render tarafında tutulur (yalnızca kendi transformuyla çizilir). Sebep: bir modelin yüzlerce düğümünü sahne dosyasına yazmak anlamsızdır ve hash/geri al'ı şişirir; elle düzenleme gerekirse prefab'a "açma" M8a'da düşünülür. Kimlik tamponu varlık düzeyindedir (Model = bir varlık); alt düğüm kimliği `describe_view`'a ek alan olarak (`node`) gelir.
- `MeshRenderer { mesh: AssetRef, material: AssetRef }` — M3'te **yok**: ilkel şekiller (küp, küre, düzlem) için `Primitive { shape: enum, size... }` + `Material` (taban renk, metal, pürüz) bileşeni testleri ve vitrin öncesi sahneleri glTF'siz kurabilmek için gerekli. → `Primitive` ve `Material` bileşenleri M3'tedir; `.mat.json` dosyası ADR-013'ün özel gölgelendirici için olan parçası sonraya.
- Kaynaklar: `Sky { zenith: Color, horizon: Color, ground: Color, sun_disk: bool }`, `Ambient { color: Color, intensity }`, `Exposure { ev100 }`.

## 4. Render mimarisi

```
ECS dünyası ─(çıkarım, salt okunur)→ RenderScene (CPU: kameralar, ışıklar, çizim listesi, kimlik haritası)
RenderScene + Model/Texture önbelleği ─→ Renderer (wgpu)
   geçişler (render grafiği düğümleri): Gölge → Opak PBR → Gökyüzü → Ton eşleme → [Hata ayıklama: Kimlik | Derinlik | Normal | Tel kafes] → Sunum | Yakalama
```

- **Çıkarım (`extract`)** ECS'i salt okur ve `RenderScene` üretir: görünür çizimler `(model/ilkel, dünya matrisi, materyal, varlık kimliği)`. Sıra sahne kimliğine göre sabit (hata ayıklama çıktıları tekrarlanabilir olsun). Render tarafı ECS'e **yazmaz**.
- **Render grafiği:** düğüm = geçiş (girdi/çıktı doku bildirimi), kenarlar kaynak kullanımı. M3'te grafik küçük ve doğrusaldır ama düğüm arayüzü (`Pass { reads, writes, run }`) baştan vardır ki gölge/ardıl/yan çıktı geçişleri sonradan eklenebilsin (ADR-012). Yan çıktılar yalnızca istenince çalışır.
- **Hedefler:** ana renk `Rgba16Float` (HDR) → ton eşleme (Khronos PBR Neutral; ACES değil: gri tonlama yan etkisi yok, glTF örnekleriyle karşılaştırmaya uygun) → `Rgba8UnormSrgb`. Kimlik `R32Uint`, derinlik `Depth32Float`, normal `Rgba16Float` (veya oktaedral `Rg16Float` — WP6'da ölçülür).
- **PBR:** glTF 2.0 metal-pürüz modeli (taban renk, metal-pürüz, normal, occlusion, emissive; `KHR_materials_emissive_strength`, `KHR_texture_transform` M3'te desteklenir; diğer uzantılar `unsupported_extension` **uyarısıyla** atlanır, sessizce yok sayılmaz). Işık: tek yönlü ışık + sabit ortam + prosedürel gökyüzü yansıması yaklaşığı.
- **Gölge:** yönlü ışık için tek gölge haritası, sahne sınırlarına oturtulmuş ortografik kesit (2048², PCF 3×3, eğim-ölçekli yanlılık). **Basamaklı** gölge (CSM) ADR-012 taban çizgisindedir ama büyük açık dünya ihtiyacı M7 vitrin oyununda ölçülür; M3'te arayüz basamak sayısına hazır, uygulama 1 basamak.
- **Tel kafes:** `POLYGON_MODE_LINE` özelliği her bağdaştırıcıda (özellikle yazılım) yok; bu yüzden ağdan türetilen kenar indeks tamponuyla `LineList`. Her yerde çalışır.
- **Hata ayıklama görünümleri** (`lit | ids | depth | normals | wireframe`): her biri ayrı çıktı; `ids` ve `depth` ayrıca sayısal olarak okunabilir (`describe`).

## 5. Gölgelendirici dili ve hataları (ADR-013 doğrulaması)

ADR-013 "M3 başında WESL olgunluğu doğrulanır". Durum: `wesl` crate'i 0.6 (derleyici), `naga` 30 WGSL ayrıştırıcı + doğrulayıcı. **WP1'de doğrulanacak:** (a) `wesl` ile `import` çözümü ve hata konumları, (b) `naga` hata aralıkları (span) → satır/sütun. Karar kuralı: WESL derleyicisi konumlu hata veriyorsa ve bağımlılık ağacı lisans listesine uyuyorsa WESL; değilse **kendi basit `#import "yol"` ön işlemcimiz** (modül sayısı M3'te 4–6, ön işlemci ~100 satır; kaynak haritası satır eşlemesi tutar) + naga. Her iki durumda hata `Diagnostic`: `shader_syntax`/`shader_validation`, dosya + satır + sütun + kaynak satırı alıntısı + ipucu. Gölgelendirici dosyaları `crates/wish_render/shaders/*.wgsl` içinde gömülü (`include_str!`) ve ayrıca geliştirmede dosyadan sıcak yüklenebilir (WP7). Kullanıcı gölgelendiricisi (`*.mat.json`) M3 kapsamı dışı.

## 6. Başsız render ve CI

- **Bağdaştırıcı seçimi:** `Renderer::new(RenderOptions { backend, software: bool })`. `software: true` → `force_fallback_adapter` (Windows: WARP üzerinden DX12; Linux: lavapipe/Vulkan — CI'da `mesa-vulkan-drivers` kurulur; macOS CI M3'te yok). Gerçek GPU yoksa ve yazılım yoksa **net tanı** (`no_gpu_adapter`, ipucu: yazılım bağdaştırıcısı/`--software`), çökme yok.
- **Altın görüntü:** PNG; karşılaştırma `max kanal farkı ≤ T` ve `farklı piksel oranı ≤ P` (başlangıç T=2/255 yazılım–yazılım, P=%0,5; WP1'de lavapipe ↔ WARP farkı **ölçülür** ve eşikler ona göre konur). Altınlar platform başına ayrı klasör gerekirse (`golden/windows`, `golden/linux`) ayrı tutulur. Sapma raporu: fark görüntüsü + sayısal özet; `UPDATE_GOLDEN=1` ile yenilenir, diff incelenir (M1'in `TRYBUILD=overwrite` düzeni gibi).
- **Risk — yazılım render sürücüler arasında birebir aynı olmaz.** Azaltma: eşik + deterministik sahne (rastgelelik yok), tek örnekleme, bilinen basit sahneler. Bu risk WP1'in **çıkış koşuludur**: iki platformda aynı sahne eşik içinde aynı çıkmıyorsa M3 planı revize edilir (kullanıcıya haber verilir).

## 7. İçe aktarma (`wish_import`)

`gltf` crate'iyle GLB/glTF: ağ (konum, normal, teğet, UV0, indeks; teğet yoksa mikktspace benzeri üretim — WP4'te `gltf` crate'inin sağladığı/ayrı küçük kütüphanenin lisansı denetlenir), materyal, doku (`image` ile PNG/JPEG çözme; KTX2 M3'te yok), düğüm ağacı, birim/koordinat dönüşümü gerekmez (ADR-021: glTF ile aynı). Hatalar `Diagnostic` (`gltf_parse`, `unsupported_extension`, `missing_buffer`, `image_decode`) dosya yolu ve (mümkünse) JSON işaretçisiyle. `*.meta.json` `import` ayarları: `scale`, `generate_tangents`, `flip_uv_y` — WP4'te yalnızca bunlar (M2 meta altyapısı kullanılır, bilinmeyen anahtar `unknown_key`).

Varlık yolu çözme `AssetRef` (M2) ile: `Model.asset = "assets/models/crate.glb"`; var olmayan dosya `wen check`'te `asset_not_found`.

## 8. Komut satırı (geçici ev sahibi; M4'te `wish_control`'e taşınır)

| Komut | Ne yapar |
|---|---|
| `wen capture --example E --scene S [--root R] [--camera ID] [--size 1280x720] [--view lit\|ids\|depth\|normals\|wireframe] [--software] --out dosya.png [--json]` | Sahneyi yükler, çizer, PNG yazar; `--json` ile `{ok, path, size, adapter, hash}` (hash: PNG baytları; GPU'ya bağlı, determinizm iddiası **değil**) |
| `wen describe --example E --scene S [--camera ID] [--at x,y] [--json]` | Kimlik tamponundan: `--at` ile o pikseldeki varlık (sahne kimliği, düğüm); yoksa görünen her varlığın ekran kutusu, görünürlük yüzdesi, kameraya uzaklık (`describe_view`'ın çekirdeği) |
| `wen view --example E --scene S` | Pencere açar (WP8) |

## 9. Test planı (çıkış ölçütleriyle)

| Çıkış ölçütü | Kanıt |
|---|---|
| Standart glTF örnek modelleri doğru görünür | Khronos glTF-Sample-Assets'ten küçük bir küme (Box, Duck, DamagedHelmet, MetalRoughSpheres, NormalTangentTest …) başvuru görüntüleriyle karşılaştırma + insan gözü incelemesi (altın görüntü değil: başvurular farklı renderer'larla üretilmiş, birebir eşleşmez). **İndirme gerektirir → kullanıcı onayı** (§12); onaya kadar `wish_import` testleri depoda üretilen küçük glTF'lerle (kod ile yazılmış Box, üçgen, dokulu düzlem) çalışır |
| `wen capture` penceresiz; altın görüntü CI'da yazılım bağdaştırıcısıyla geçer | CI'da Windows (WARP) + Linux (lavapipe) matrisinde `cargo test` içindeki altın testleri; `wen capture` entegrasyon testi |
| Kimlik tamponundan "şu pikselde hangi varlık" doğru | Sahnede bilinen konumlu 3 küp: piksel → varlık eşlemesi; arka plan = yok; örtülme (öndeki kazanır); kısmi görünürlük yüzdesi |
| Gölgelendirici hatası ortak tanı formatında, dosya ve satırla | Bozuk gölgelendirici örnekleri (sözdizimi, tür, olmayan içe aktarma) → `Diagnostic` dosya/satır/sütun/ipucu; M1/M2'deki "bozuk dosya" test desenini izler |
| (ek) Render ECS'e yazmaz, hash'i değiştirmez | Çizimden önce/sonra dünya hash'i eşit |
| (ek) GPU yokken çökme yok | Bağdaştırıcı yoksa `no_gpu_adapter` tanısı testi (yazılım zorlanmadan, kasıtlı geçersiz backend ile) |

## 10. İş paketleri ve önerilen model

| # | İş paketi | Model · efor |
|---|---|---|
| WP1 | **Kanıt işi (spike):** wgpu başsız + yazılım bağdaştırıcısı Windows/Linux CI'da ışık-gölge-yok üçgen çizip PNG yazar; iki platform farkı ölçülür ve eşikler belirlenir; WESL/naga hata konumu doğrulaması (ADR-013 kararı); yeni bağımlılıkların lisans taraması (`deny.toml`). **Çıkış koşulu:** §6 riski | Opus 5.5 · high |
| WP2 | `wish_transform` (Transform, GlobalTransform, yayılım) ; sahne bileşenleri (`Camera`, `DirectionalLight`, `Primitive`, `Material`, `Model`, kaynaklar); çıkarım (`RenderScene`) | Sonnet 5.5 · high |
| WP3 | Render çekirdeği: aygıt/kaynak yönetimi, render grafiği iskeleti, PBR (dokusuz), gökyüzü, ton eşleme, yakalama + PNG, `wen capture` | Opus 5.5 · high |
| WP4 | `wish_import`: glTF → `Model`, dokular, materyal uzantıları, teğet; render'da dokulu PBR | Sonnet 5.5 · medium |
| WP5 | Yönlü ışık gölgesi (tek gölge haritası, PCF) | Sonnet 5.5 · high |
| WP6 | Hata ayıklama görünümleri: kimlik, derinlik, normal, tel kafes; `wen describe`; görünürlük yüzdesi | Sonnet 5.5 · high |
| WP7 | Gölgelendirici tanıları + (geliştirmede) gölgelendirici sıcak yükleme | Sonnet 5.5 · medium |
| WP8 | `wish_window` + `wen view` (ince); M3 kapanış incelemesi (bağımsız ajan) + M3 sonucu | Opus 5.5 · high (inceleme), Sonnet 5.5 · medium (pencere) |

## 11. Riskler

1. **Yazılım render farkı** (§6) — WP1 çıkış koşulu.
2. **Bağımlılık ağırlığı:** `wgpu` ağacı geniştir (derleme süresi, lisans çeşitliliği). WP1 `cargo-deny` ile ölçer; izin listesi dışı lisans çıkarsa durur ve bildirir (yeni lisans eklemek kullanıcı kararıdır).
3. **Derleme süresi/CI süresi:** `wgpu` ilk derlemede dakikalar ekler; CI önbelleği (`Swatinem/rust-cache`) WP1'de eklenir.
4. **Kapsam büyümesi:** CSM, IBL, nokta ışıklar cazip; M3 "ilk piksel"dir, bunlar listeye yazılır (§1).

## 12. Kullanıcıya ait kararlar / onaylar

- **glTF örnek modellerinin indirilmesi** (Khronos glTF-Sample-Assets, birden çok lisans: çoğu CC-BY/CC0/SI; depoya girmeden yalnızca yerelde test için indirilecek, depoya **eklenmeyecek**): kaynağı, dosya adları ve boyutu söyleyerek WP4 sonunda onay istenecek.
- Yeni bir dış bağımlılığın izin listesi dışında lisansı çıkarsa: lisans listesine ekleme onayı.
- Kapsam: bu notta "M3'te yok" denen her şey (animasyon, IBL, CSM, nokta ışık, KTX2) **1.0 kapsamı içindedir**, sonraki kilometre taşlarına yazılmıştır; kapsamı büyüten bir değişiklik olursa burada belirtilir.

## 13. Açık sorular

1. `Transform.rotation` için `euler_deg` kısa yazımı (ajan/insan kolaylığı) M4'te sorulacak.
2. Normal tamponu biçimi (oktaedral `Rg16Float` mı tam `Rgba16Float` mı) WP6'da ölçülür.
3. Teğet üretimi için kütüphane seçimi (WP4).
