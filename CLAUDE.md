# Wish Engine — Motoru Geliştiren Ajan İçin Çalışma Kuralları

Wish Engine: yapay zekâya tamamen entegre ama yapay zekâsız da eksiksiz kullanılabilen (hibrit), açık kaynak, ücretsiz ve komisyonsuz dağıtılacak, MCP-yerel, deterministik bir 3D oyun motoru. Rust ile yazılır.

## Şu anki durum

- **Tamamlanan:** M0 (tasarım), **M1 (çekirdek)** — 15 Kasım 2026. Çıkış ölçütlerinin hepsi karşılandı (CI dahil: depo GitHub'a gönderildikten sonra Windows ve Linux'ta yeşil). Ayrıntı, kapanış incelemesi bulguları ve M2'ye devredilenler: [docs/design/M1-cekirdek.md](docs/design/M1-cekirdek.md) §12 "WP8 ve M1 kapanışı".
  - Crate'ler: `wish_core` (kimlikler, tanı, kararlı hash, `Rng`, öneriler), `wish_schema` + `wish_schema_macros` (şema, `Plain`, JSON ⇄ bayt, JSON Schema), `wish_ecs` (dünya, sorgular, komutlar, olaylar, hiyerarşi, değişiklik izleme, anlık görüntü, hash), `wish_app` (sabit adım, zamanlayıcı), `wish_examples` (`bounce`, `stress`), `wish_cli` (`wen`). M2'de eklenenler: `wish_scene` (belge, işlemler, prefab'lar, dünyaya aktarım), `wish_asset` (varlık dizini, meta, başvurular, `wen mv`).
- **Tamamlanan: M2 (veri katmanı)** — 15 Kasım 2026, `m2-data` dalından `main`'e birleştirildi. Çıkış ölçütleri ve kanıtları, kapanış incelemesi (9 bulgu, hepsi düzeltildi): [docs/design/M2-veri-katmani.md](docs/design/M2-veri-katmani.md) §12 "WP8" ve "M2 çıkış ölçütleri". Sahne/prefab/meta dosyalarına ya da `wish_scene`/`wish_asset`'e dokunmadan önce o notun §12'sini oku.
  - Komutlar: `wen run --example <ad> [--scene <dosya> --root <klasör>]`, `wen check`, `wen fmt [--check]`, `wen migrate`, `wen watch`, `wen mv`. Proje komutları bileşen türlerini `--example`'dan alır (oyun modülleri M5'te).
  - Makro hata iletileri `trybuild` ile sabitlendi (`wish_schema/tests/ui/*.stderr`). Yeni bir `Plain` türü eklemek ya da makronun iletisini değiştirmek bu dosyaları değiştirir: `$env:TRYBUILD = "overwrite"; cargo test -p wish_schema --test compile_fail`, sonra farkı incele.
- **Tamamlanan: M3 (ilk piksel: wgpu render temeli, glTF, başsız yakalama, pencere)** — 15 Kasım 2026, `m3-render` dalından `main`'e birleştirildi. Kapanış incelemesi ve çıkış ölçütü kanıtları: tasarım §14 "M3 kapanışı". Tasarım notu yazıldı: [docs/design/M3-ilk-piksel.md](docs/design/M3-ilk-piksel.md) — M3'te kod yazmadan önce oku (§2 crate'ler, §6 başsız render riski, §10 iş paketleri ve modeller).
  - M3 WP1 (kanıt işi) bitti: `wish_render` (`Gpu`, başsız yakalama, PNG, altın görüntü karşılaştırması, `check_wgsl`); WARP ↔ lavapipe farkı en çok 1/255; ADR-013 kararı: `naga` + kendi içe aktarma ön işlemcisi, WESL sonra (§14).
  - M3 WP2 bitti: `wish_transform` (`Transform`, `GlobalTransform`, yayılım), render bileşenleri/kaynakları (`Camera`, `DirectionalLight`, `Primitive`, `Material`, `Model`, `Sky`, `Ambient`, `Exposure`), `extract` → `RenderScene`; `wen` motor türlerini tanır (`wish_cli::types`).
  - M3 WP3 bitti: `ShaderLibrary` (`#import`, hatalar modüle eşlenir), `Renderer` (gökyüzü → PBR → ton eşleme), ilkel ağlar, `wen capture`, örnek proje `examples/first-pixel`. RTX ↔ WARP farkı en çok 1/255.
  - **GPU kuralları (WP3 bulguları, çiğneme):** tüm GPU işi `gpu_lock` altında ve `Gpu::checked` içinde; süreç başına `Gpu::shared`; `create_buffer_init` kullanma (panikleyebilir) → kuyrukla doldur; gölgelendiricilerde sıfıra bölme ve düzleşince sıfıra bölen erken `return` yazma; render testleri release'te de geçmeli (`cargo test --release -p wish_render`), çünkü debug'da `wgpu` sürücü hata ayıklama katmanını açar ve hataları gizleyebilir.
  - M3 WP4 bitti: `wish_import` (glTF/GLB → CPU verisi; desteklenmeyenler uyarı), render'da dokulu PBR (normal haritası, occlusion, emissive, alfa MASK, çift taraflı) ve `Model` çizimi, `wen capture` modelleri projeden yükler. **`gltf-json` hostile dosyada panikler**: çağrı `catch_unwind` içinde (`wish_import::guarded`), kaldırma.
  - M3 WP5 bitti: yönlü ışık gölgesi (2048² tek harita sahneye oturtulmuş, 3×3 PCF, normal ofseti). Gölge testleri **gölgeli/gölgesiz karşılaştırır** ve gölge *ucundan* ölçer (nesnenin altındaki nokta görüntüde nesnenin arkasında kalabilir).
  - **Khronos örnek modelleri** (Box, Duck, DamagedHelmet, MetalRoughSpheres, NormalTangentTest) kullanıcı onayıyla `C:\wish-build\khronos-samples` içine indirildi (depoda DEĞİL, lisanslar farklı); `wish_render/tests/khronos.rs` bunları görsel inceleme için çizer (`#[ignore]`). Gerçek modeller bitanjant işareti hatasını buldu; render/ışık/doku değişikliklerinden sonra galeriye bakmak iyi bir son kontrol.
  - M3 WP6 bitti: hata ayıklama görünümleri (`wen capture --view lit|ids|depth|normals|wireframe`), `IdBuffer` ("bu pikselde hangi varlık"), `wen describe` (görünür varlıklar, ekranda görünen yüzde, ekran kutusu, uzaklık, gizliler). Kod `wish_render/src/renderer/debug.rs` + `shaders/debug_pass.wgsl`; görünümler gerçek çizimle aynı çizim listesini kullanır. Altın: `view-ids/depth/normals` (wireframe yapısal test). Ayrıntı: tasarım §14 WP6.
  - M3 WP7 bitti: `Renderer::reload_shaders` (hata olursa eski gölgelendirici kalır, modeller kalır), `engine_shaders_from_dir`, `wen shaders [--dir]`, `wen capture/describe --shaders`. Ayrıntı: tasarım §14 WP7.
  - M3 WP8 bitti: `wish_window` (winit 0.30 + softbuffer; renderer ekran dışına çizer, pencere resmi gösterir; wgpu yine yalnızca `wish_render`'da), `wen view` (uçuş kamerası, 1–5 görünümler, tıkla seç, sahne/model/gölgelendirici sıcak yükleme). Pencere döngüsü CI'da çalışmaz: yerelde `wen view ... --exit-after 3` ile dene. Ayrıntı: tasarım §14 WP8.
  - M3'ten M4'e devirler: `capture/describe/shaders/view` komutlarını `wish_control` sarmalı; `wen watch` yarım dosyayı bildirmeden önce kısa kararlılık beklesin; açık: örnekleme için ağ kopyası (M6), aygıt kaybı kurtarma (M5). **Hostile girdi kuralı:** glTF gibi dış kütüphane çağrılarının hepsi `guarded` içinde, düğüm ağacı orman olmalı, `placed()` bütçeli.
  - Render testleri yazılım bağdaştırıcısı ister: yerelde WARP hazır; altın görüntüler `UPDATE_GOLDEN=1` ile yenilenir, farkı gözle incele.
- **Tamamlanan: M4 (kontrol düzlemi + MCP)** — 16 Kasım 2026, `m4-control` dalından `main`'e birleştirildi. Çıkış ölçütleri ve kanıtları: [docs/design/M4-kontrol-duzlemi.md](docs/design/M4-kontrol-duzlemi.md) §15 "M4 sonucu"; kapanış incelemesi (2 yüksek + 7 orta bulgu, hepsi düzeltildi, regresyon testleri `wish_control/tests/review_m4.rs`, `wish_mcp/tests/serve.rs`): §14 WP8. Araç eklerken ya da `wish_control`/`wish_mcp`'ye dokunurken önce §3, §5, §6 ve §14'ü oku. Kararlar: MCP'yi kendimiz uyguluyoruz (rmcp/tokio yok), protokol 2025-06-18; HTTP M8a'ya kaydı; araç parametreleri tek tablodan şema + ayrıştırıcı + yardım üretir.
  - M4 WP1 bitti: `wish_control` (`Workspace::call` → zarf, asla panik dışarı sızmaz), `Param` tablosu (şema + ayrıştırıcı + bayraklar), `wish.json`, `project_info`/`schema_get`/`check`, `wen call` / `wen do`, eşlik testi (`wish_cli/tests/parity.rs`: yeni araç = yeni örnek, yoksa test kırılır). Ayrıntı: tasarım §14 WP1.
  - M4 WP2 bitti: `scene_get`, `entity_query` (+ `expr.rs` ifade dili), `entity_get`, `edit_apply` (`ops.rs`, `euler_deg`), `edit_history`; `documents.rs`: atomik kanonik yazma, dışarıdan dosya değişikliği `Actor::File` işlemi olarak tek geçmişe girer, **bozuk elle düzenlemenin üzerine asla yazılmaz** (`scene_file_invalid`). Eşlik testi durumlu (tek oturum: süreç içi = `wen do`). Ayrıntı: tasarım §14 WP2.
  - M4 WP3 bitti: `asset_list`/`asset_info`/`asset_move`, `wen new` (`wish_control::scaffold`, `.mcp.json` tam yolla, proje `CLAUDE.md`), `wen check`/`wen mv` artık araç çağırır (`wen check --json` → `passed`). Geçici dosyalar (`.part` vb.) varlık/izleme dışı. Ayrıntı: tasarım §14 WP3.
  - M4 WP4 bitti: oturumlar (`sessions.rs`, `tools/sim.rs`): `sim_start/step/snapshot/restore (fork)/stop`, `logs_get`, `until` (kaynak + varlık koşulu), olay sayımı (`World::events_sent`), denetçi; `entity_query/get` `session` alır. Aynı sahne+tohum → aynı hash (süreçler arası da). `edit_apply` oturuma M5'te. Ayrıntı: tasarım §14 WP4.
  - M4 WP5 bitti: `capture_frame` (PNG base64 ya da proje içi `out`; asla belgenin üzerine yazmaz), `describe_view` (nearest-first); sahne ve oturumda; GPU tembel açılır (`Workspace::set_software`, `WISH_SOFTWARE`); `wen capture/describe` artık araç çağırır; PNG baytları süreçler arası aynı. Ayrıntı: tasarım §14 WP5.
  - M4 WP6 bitti: `wish_mcp` (stdio JSON-RPC, 2025-06-18; araçlar, `wish://` kaynakları, 2 istem), `wen mcp`, `wish_control::docs` (kılavuz, tür sayfaları, `DIAGNOSTICS` kataloğu, `docs_search` = 20. araç). **Yeni bir tanı kodu eklersen `docs::DIAGNOSTICS`'e de ekle** (sıralı; `tests/diagnostics_catalog.rs` kaynağı tarar ve eksiği söyler). Eşlik testinin MCP ayağı ve çıkış ölçütünün betikli uçtan uca testi `wish_cli/tests/parity.rs`'de. Ayrıntı: tasarım §14 WP6.
  - M4 WP7 bitti: `crates/wish_evals` (`wish-evals selftest | run | recheck`), 10 görev; Claude Code CLI kullanıcı onayıyla kuruldu (abonelikle oturum açık; koşumlar kotadan harcar, her `run` öncesi kullanıcı onayı gerekir). Sonuç: Sonnet 5.5 · medium **30/30 iki kez** (~2,4 $ karşılığı/koşu); bulgular düzeltildi (prefab okuma, `concepts.rs` kavram ipuçları, override ipucu, `640.0`). Ayrıntı: tasarım §14 WP7, `evals/results/`.
  - M4 WP8 bitti: bağımsız kapanış incelemesi ve düzeltmeleri. **Kurallar (çiğneme):** belge değişikliği kopyada yapılır, yalnızca yazma başarılıysa belgeye konur (`commit_document`); `changed_by_others` yalnızca başarılı çağrıdan sonra işaretlenir; araçların yazdığı yollar beyaz listeyle (`normalize_output_path` + `inside_project`), sahne yolları tam harf yazımıyla (`exact_case`); var olan dosyanın üzerine yazmak açık parametre ister. `sim_step` iptal edilebilir (`notifications/cancelled`, tick sınırında).
  - Değerlendirme koşumu (`wish-evals run`) Claude kullanımı harcar: **koşmadan önce kullanıcıdan onay al.** `wish-evals selftest` ücretsizdir.
  - M4'ten devirler: `edit_apply` canlı oturuma ve oyun kodu değerlendirme görevleri (M5); `until` maliyeti (M6); HTTP, `wen view/watch/shaders` araçlaşması, prefab düzenleme (M8a). Ayrıntı: tasarım §15.
- **Süren: M5 (oyun mantığı: Wasm ev sahibi, SDK, sıcak yükleme, girdi, senaryo testleri)**, `m5-logic` dalında. Tasarım notu: [docs/design/M5-oyun-mantigi.md](docs/design/M5-oyun-mantigi.md) — M5'te kod yazmadan önce oku (§3 ABI, §5 derleme yolu, §6 korumalı alan, §13 iş paketleri ve modeller). `wasmtime` yalnızca `wish_script`'te; konuk yalnızca `wish` içe aktarmalarını kullanabilir; yakıt belirlenimcidir, duvar saatiyle kesme yok.
  - **Sıradaki: M5 WP1 (kanıt işi)** — `wasm32-unknown-unknown` hedefinin indirilmesi kullanıcı onayı bekliyor. Önerilen model · efor: Opus 5.5 · high.
- **`wen`'i çalıştırmak:** `cargo run -p wish_cli --release -- run --example bounce --frames 10000 --seed 7 --json`. Derlenmiş ikili `C:\wish-build\target\release\wen.exe` altında.
- **Hash'leri değiştirme konusunda dikkat:** `wish_ecs/tests/snapshot.rs` ve `wish_core` testlerinde kayıtlı hash/rastgelelik değerleri var. Bunlar kırılırsa önce bilinçli bir format değişikliği olup olmadığını düşün; bilinçliyse sürüm etiketini (`wish-world-hash-v1` vb.) artır.
- **Git:** M1 (`m1-core`) ve M2 (`m2-data`) kendi dallarında geliştirilip `main`'e birleştirildi. Her kilometre taşı kendi dalında (`m2-...`) yürür, bitince `main`'e birleştirilir. Commit kimliği depoya özel proje kimliğidir (kullanıcı kararı); kişisel e-posta kullanma.
- **Ortam (15 Kasım 2026'da doğrulandı):** Rust 1.100.0 (`rust-toolchain.toml` ile sabit) ve Visual Studio 2022 Build Tools 17.14.45 kurulu. Bu oturumun kabuğunda `cargo` PATH'te olmayabilir; her komuttan önce `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"` çalıştır.
- **Proje klasörü OneDrive altında** (`Masaüstü\wish-engine`). Derleme çıktıları git'e girmeyen yerel `.cargo/config.toml` ile `C:/wish-build/target` klasörüne yönlendirildi. Bu dosyayı silme; `target/` klasörünü asla proje klasörünün içinde bırakma.
- **Uzak depo ve CI:** özel GitHub deposu `akeremkar-sketch/wish-engine` (kullanıcının hesabı, kullanıcı kararı 15 Kasım 2026; `origin`). CI `main`'e gönderimde ve PR'larda çalışır (Windows + Linux: fmt, clippy, test; cargo-deny lisans). İlk çalıştırma yeşil; kayıtlı hash ve rastgelelik değerleri Linux'ta da tuttu. Dalları gönderirken `git push`; `main`'e yalnızca kilometre taşı bitince birleştir.

## Önce oku

| Ne zaman | Belge |
|---|---|
| Her oturum başında | Bu dosya + [docs/ROADMAP.md](docs/ROADMAP.md) içindeki etkin kilometre taşı |
| Tasarım kararı gerektiğinde | [docs/DECISIONS.md](docs/DECISIONS.md) |
| Parçaların nasıl birleştiğini görmek için | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| MCP/komut satırı yüzeyine dokunurken | [docs/MCP_API.md](docs/MCP_API.md) |
| "Bu neden var?" sorusunda | [docs/VISION.md](docs/VISION.md) |

## Kullanıcıyla çalışma

- Kullanıcıyla **Türkçe** konuş.
- **Her kilometre taşına (ve belirgin biçimde farklı her iş paketine) başlamadan önce önerilen model ve efor değerini söyle**; yol haritasındaki tabloyu esas al, gerekiyorsa gerekçesiyle güncelle. Kullanıcı bunu açıkça istedi.
- Kullanıcının vizyonu: her şey **sorunsuz** olmalı ve seçimlerde **ileriyi gören, yenilikçi** seçenek tercih edilmeli. İki seçenek arasında kalırsan bunu ölçüt al; ama kapsamı büyüten her öneriyi açıkça "bu 1.0 kapsamını büyütür" diye belirt.
- Kullanıcıya yalnızca gerçekten onun vermesi gereken kararları sor (ad, lisans, para/hesap gerektiren işler). Teknik kararları gerekçesiyle kendin ver ve belgeye yaz.
- **İş paketleri arasında onay bekleme** (kullanıcı kararı, 15 Kasım 2026: "bana sormadan ilerle"). Bir paketi doğrula, commit et, kısa durum yaz ve sıradakine geç. Durmak için yalnızca kullanıcıya ait karar, yeni indirme/kurulum onayı ya da geri alınamaz işlem gerekçe olabilir.

## Değişmezler (çiğnenemez)

Bir değişiklik bunlardan birini bozuyorsa değişiklik yanlıştır. Gerekçeler [docs/DECISIONS.md](docs/DECISIONS.md) içinde.

1. **Şema önce.** Şeması olmayan tür dünyaya giremez. Her alanın belge yorumu, uygunsa birimi ve aralığı olur.
2. **Determinizm.** Simülasyon kodunda duvar saati, tohumsuz rastgelelik, sırasız koleksiyon üzerinden yineleme, iş parçacığı zamanlamasına bağlı sonuç **yasak**.
3. **Dosyalar doğruluk kaynağıdır.** Kanonik biçim; yükle → kaydet bayt düzeyinde aynı sonucu verir.
4. **Tek kontrol düzlemi.** Yeni yetenek önce `wish_control` içine eklenir; MCP, komut satırı ve arayüz onu çağırır. Yalnızca arayüzde bulunan yetenek olamaz.
5. **Tek tanı formatı.** Her hata `code`, `message`, konum ve **`hint`** taşır. İpucu yazılmamış hata tamamlanmamış sayılır.
6. **Bağımlılık yönü aşağı.** Alt sistemler birbirini tanımaz; ECS üzerinden konuşur. Dış kütüphaneler yalnızca kendi sarmalayıcı crate'lerinden kullanılır.
7. **Oyun kodu güvenilmezdir.** Wasm konuğu motoru çökertememeli, donduramamalı; durum konukta tutulmaz.
8. **Bir özellik; kontrol düzleminden gözlemlenebilir/denetlenebilir olmadan ve başsız testi yazılmadan bitmiş sayılmaz.**
9. **Hibrit eşlik.** Motor, hiçbir yapay zekâ bağlı değilken eksiksiz çalışır. Ajanın yapabildiği her sahne/varlık işlemi arayüzden elle de yapılabilir. Elle ve ajanla yapılan değişiklikler aynı geçmişe, kimin yaptığı bilgisiyle (`actor`) girer.

## Çalışma yöntemi

1. Kod yazmadan önce kısa bir tasarım notu yaz: `docs/design/Mx-konu.md` (sorun, seçenekler, karar, test planı).
2. Küçük adımlarla ilerle: her adım derlenir, test edilir, commit edilir.
3. "Bitti" deme, **göster**: çıkış ölçütünü kanıtlayan komutu çalıştır ve çıktısını raporla. Test başarısızsa bunu çıktısıyla birlikte söyle.
4. Bir ADR'yi değiştirmen gerekiyorsa önce [docs/DECISIONS.md](docs/DECISIONS.md) dosyasını güncelle, sonra kodu.
5. Kilometre taşı bitince [docs/ROADMAP.md](docs/ROADMAP.md) ve bu dosyadaki "Şu anki durum" bölümünü güncelle.
6. Kapsam dışı bir fikir çıkarsa uygulama; M10 listesine not düş.

## Kod uzlaşımları

- Kod, tanımlayıcılar, API adları, tanı mesajları, kod yorumları ve commit iletileri **İngilizce**. Planlama belgeleri Türkçe.
- Crate öneki `wish_`; komut satırı aracı `wen` (`wish` değil: o ad Tcl/Tk'nin komutudur ve çakışır).
- Lisans: MIT + Apache-2.0 (açık kaynak). Her crate'in `Cargo.toml` dosyasında `license = "MIT OR Apache-2.0"`.
- `cargo fmt` ve uyarısız `cargo clippy` zorunlu.
- `unsafe` yalnızca `wish_ecs` ve `wish_script` içinde, bir de `wish_schema`'daki `Plain` sözleşmesinin bildiriminde (ve makronun ürettiği `unsafe impl Plain`'de); her blokta neden güvenli olduğunu açıklayan `// SAFETY:` yorumu; Miri ile test. Diğer her yerde çalışma alanı lint'i `unsafe_code = "deny"`.
- Kütüphane kodunda `unwrap`/`expect`/`panic!` yok (testler hariç); hatalar `Diagnostic` olarak döner. Bunu clippy zorlar (`Cargo.toml` içindeki `[workspace.lints]`).
- Simülasyon kodunda `HashMap`/`HashSet` üzerinde gezinme yasak (clippy `iter_over_hash_type`); sıralı gezinme gerekiyorsa `BTreeMap` ya da `Vec` kullan.
- Kararlı hash gereken her yerde (dünya hash'i, parmak izi, diske yazılan her şey) `wish_core::StableHasher` kullan; standart kitaplığın hash'leyicilerini asla.
- Dünyaya giren türler yalnızca `#[component]`, `#[resource]`, `#[event]`, `#[schema]` ile tanımlanır; `Plain` elle uygulanmaz. Bu makrolar `Clone`, `Copy`, `Debug`, `PartialEq`, `Default` ve `repr`'ı kendileri ekler (bunları ayrıca yazmak derleme hatasıdır).
- `wish_ecs` sütunlarındaki bir değeri asla bütün olarak `&[u8]` diye okuma: dolgu baytları başlatılmamış olabilir. Bayt gerekiyorsa `TypeInfo::spans` ile yalnızca alan aralıklarını kopyala (`Column::read_into`); değer taşımak için ham kopya kullan.
- Motorun tutarlılığını kendisinin sağladığı bileşenler (şu an yalnızca `Hierarchy`) kayıtta `managed` işaretlidir. Yeni bir yazma yolu eklersen yönetilen bileşenleri reddettiğinden emin ol (`World::check_writable`); motorun kendi iç yazmaları `put_component` kullanır.
- Hata döndüren işlevlerde `wish_core::Result<T>` kullan. `Diagnostic` tek işaretçi genişliğindedir; onu büyütecek bir değişiklik yapma (bir test bunu denetler).
- Bütünleşme testi dosyalarının (`tests/*.rs`) başına `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` koy: clippy yalnızca `#[test]` işlevlerini test sayar, ortak yardımcıları saymaz.
- Herkese açık her öge belge yorumu taşır (bunlar şemaya ve MCP belgelerine akar).
- Koordinat sistemi: sağ el, +Y yukarı, kamera −Z'ye bakar. Birimler: metre, saniye, kilogram; API'de radyan.
- Bağımlılık eklemeden önce lisansını denetle (yalnızca MIT/Apache-2.0/BSD benzeri serbest lisanslar).

## Doğrulama komutları

Her commit'ten önce üçü de temiz olmalı (proje kökünde, PowerShell):

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Commit iletisini her zaman dosyadan ver (`git commit -F <dosya>`): PowerShell, `-m` ile verilen iletideki çift tırnakları bozar ve commit yarıda kalır.

Bağımlılık eklediysen lisansını gör: `cargo metadata --format-version 1 | ConvertFrom-Json` çıktısındaki `packages[].license` alanı; izinli liste `deny.toml` içinde.

`unsafe` içeren bir crate'e (şu an `wish_ecs`) dokunduysan Miri de temiz olmalı (nightly kurulu; ilk çalıştırma birkaç dakika sürer, arka planda çalıştır):

```powershell
$env:MIRIFLAGS = '-Zmiri-strict-provenance'
cargo +nightly miri test -p wish_ecs
```

Determinizm testi ve kıyaslama (`wen bench`) M1'de eklendi; `cargo test --workspace` derleme hatası testlerini (`trybuild`) de çalıştırır.
