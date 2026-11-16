# Wish Engine — Yol Haritası

Her kilometre taşının **çıktısı**, **çıkış ölçütü** (kanıtlanmadan sonrakine geçilmez) ve **önerilen model · efor** değeri vardır.
Boyutlar görecelidir (S < M < L < XL); takvim sözü değildir.

## Genel bakış

| # | Kilometre taşı | Öz | Boyut | Model · efor |
|---|---|---|---|---|
| M0 | Tasarım | Vizyon, kararlar, mimari, MCP taslağı | S | Fable 5.2 · xhigh — **tamamlandı** |
| M1 | Çekirdek | Şema, ECS, sabit adım, determinizm, anlık görüntü | L | İlk oturum Fable 5.2 · high → sonra Opus 5.5 · high — **tamamlandı** |
| M2 | Veri katmanı | Sahne formatı, biçimlendirici, doğrulama, varlık sunucusu, işlem/geri al | M | Sonnet 5.5 · high (tasarım ağırlıklı iş paketleri Opus 5.5 · high) — **tamamlandı** |
| M3 | İlk piksel | wgpu render temeli, glTF, başsız yakalama | L | Opus 5.5 · high (çekirdek) → Sonnet 5.5 · medium (içe aktarıcılar) — **tamamlandı** |
| M4 | Kontrol düzlemi + MCP | Tek API, MCP sunucusu, komut satırı eşliği, ilk ajan değerlendirmeleri | M | İlk oturum Opus 5.5 · high (araç tasarımı) → Sonnet 5.5 · medium |
| M5 | Oyun mantığı | Wasm ev sahibi + SDK, sıcak yükleme, girdi, senaryo testleri, gömülü araç zinciri denemesi | L | Opus 5.5 · high (ABI, ev sahibi) → Sonnet 5.5 · medium — **tamamlandı** (iki kullanıcı doğrulaması bekliyor) |
| M6 | Oyun sistemleri | Fizik, ses, oyun içi arayüz, animasyon, 2D alt kümesi | L | Sonnet 5.5 · medium (sorunlu bütünleşmede Opus 5.5 · high) |
| M7 | Kendi motorumuzla oyun | Vitrin oyunu, ajan değerlendirme takımı, sürtünme giderme | M | Oyun yapan ajan: Sonnet 5.5 · medium ve Haiku 5.5; düzeltmeler: Opus 5.5 · high |
| M8a | Editör temeli (elle düzenleme) | Görünüm, hiyerarşi, otomatik denetçi, tutamaçlar, varlık tarayıcı, oynatma denetimleri | L | Sonnet 5.5 · high |
| M8b | Yapay zekâ kokpiti | Konuşma paneli, kokpit içi ajan, "göster ve söyle", etkinlik akışı, denetim noktaları | L | Sonnet 5.5 · high (etkileşim tasarımı kararlarında Opus 5.5 · high) |
| M9 | Ürünleştirme → **1.0** | Tek paket kurulum, dışa aktarma, ilk çalıştırma, belgeler, Steam/web | L | Sonnet 5.5 · medium; yayın denetimi Opus 5.5 · high |
| M10 | 1.0 sonrası | Gelişmiş render, web hedefi, ikinci betik dili, çok oyunculu | açık uçlu | Fable 5.2 / Opus 5.5 · high–xhigh |
| T1 | Arayüz tasarımı (paralel) | Hibrit arayüzün maketleri: elle düzenleme ve kokpit akışları, ikisi arasında geçiş, kullanıcı sınaması | S | Opus 5.5 · high — M4 ile birlikte başlar |

### Model ve efor seçme kuralı

- **Efor, işin büyüklüğüne değil belirsizliğine göre seçilir.** Tanımı net iş → medium. Tasarım kararı, `unsafe` kod, zor hata → high. Yalnızca mimari çıkmazlarda → xhigh.
- **Fable 5.2 / Opus 5.5:** sonradan değiştirmesi pahalı olan şeyler (veri yerleşimi, ABI, araç yüzeyi, determinizm).
- **Sonnet 5.5:** tasarımı belli olan uygulamanın büyük kısmı.
- **Haiku 5.5:** tekrarlayan işler (belge, test çoğaltma, toplu yeniden adlandırma) ve M7'de "zayıf modelle de çalışıyor mu?" sınaması.
- Sonnet aynı soruna iki kez takılırsa o oturumu Opus'a yükseltin; üçüncü denemeyi aynı modelle yapmayın.
- Her kilometre taşına başlarken bu tablodaki öneri yeniden söylenir ve o günün koşullarına göre güncellenir.

### Paralel yürütülebilecek işler

- **M2 ve M3** büyük ölçüde bağımsızdır (veri katmanı ve render); ayrı oturumlarda aynı anda ilerleyebilir.
- **T1 (arayüz tasarımı)** kod gerektirmez; M4 ile birlikte başlayabilir.
- **M8a (editör temeli)** yalnızca M3 ve M4'e bağlıdır; M5–M7 ile aynı anda ilerleyebilir.
- **M6** içindeki fizik, ses ve oyun içi arayüz birbirinden bağımsızdır.

---

## Başlamadan önce: geliştirme ortamı

**Durum: ortam hazır** (15 Kasım 2026'da doğrulandı). Makinede yapılan denetimin sonucu:

| Öge | Durum | Yapılacak |
|---|---|---|
| Git | Kurulu (2.55) | — |
| Rust (`rustup`, `cargo`) | **Kuruldu** (15 Kasım 2026): rustc 1.100.0, `stable-x86_64-pc-windows-msvc` | — |
| MSVC derleme araçları | **Kuruldu** (15 Kasım 2026): Visual Studio 2022 Build Tools 17.14.45, "C++ ile masaüstü geliştirme" iş yükü (MSVC v143, Windows 11 SDK 10.0.26100) | — |
| Kod düzenleyici | Visual Studio Code kurulu | Oyun kodunu elle yazmak için önerilen dış düzenleyici (ADR-018) |
| Ekran kartı | RTX 3060 Ti | Uygun: DX12, Vulkan ve donanımsal ışın izleme (M10 için de yeterli) |
| İşlemci / bellek | Ryzen 5 5600 (12 iş parçacığı) / 16 GB | Yeterli. Derleme sırasında bellek sıkışırsa paralel derleme işi sayısı düşürülür |
| Disk | C: sürücüsünde 94,9 GB boş (15 Kasım 2026; 10 Kasım'da 21 GB idi) | Yeterli. Araç zinciri + derleme çıktıları bu projede 30–40 GB'a ulaşabilir; boş alan 30 GB'ın altına düşerse derleme çıktıları temizlenir |
| Proje klasörü | `Masaüstü\wish-engine` (OneDrive altında; kullanıcı seçimi) | **Derleme çıktıları OneDrive dışına yönlendirilecek.** Rust derleme klasörü on binlerce dosya üretir; OneDrive eşitlemesi derlemeyi yavaşlatır ve dosya kilitleme hatalarına yol açar. M1'in ilk iş paketinde, git'e girmeyen yerel `.cargo/config.toml` ile `target-dir = "C:/wish-build/target"` ayarlanır. Kaynak dosyaların OneDrive'da eşitlenmesi sorun değildir |

**Doğrulama (15 Kasım 2026).** Geçici bir Rust projesi oluşturuldu; `cargo run` derleyip çalıştırdı, `cargo test` 1 testi geçirdi. Araç zinciri uçtan uca çalışıyor; M1'in önünde ortam kaynaklı engel yok.

M1'in ilk iş paketinde yapılacak tek ortam işi: derleme çıktılarını OneDrive dışına yönlendirmek (yukarıdaki "Proje klasörü" satırı).

---

## M1 — Çekirdek

**Amaç.** Üzerine her şeyin kurulacağı deterministik, şemalı dünya.

**İş paketleri (sırayla).**
1. Çalışma alanı: crate iskeleti, `rust-toolchain.toml`, CI (biçim, clippy uyarısız, testler, bağımlılık lisans denetimi), git deposu, `LICENSE-MIT` ve `LICENSE-APACHE` dosyaları.
2. `wish_core`: kimlikler, `Diagnostic`, kararlı hash.
3. `wish_schema`: `TypeSchema` modeli, `#[component]` türetme makrosu, JSON ⇄ ikili dönüşüm, JSON Schema üretimi.
4. `wish_ecs`: varlıklar, arketip depolama, sorgular, ertelenmiş komutlar, kaynaklar, olaylar, değişiklik izleme, hiyerarşi.
5. Zamanlayıcı: açık sistem sırası, sabit adım, tohumlu `Rng`.
6. Anlık görüntü / geri yükleme ve dünya hash'i.
7. `wish_cli`: `wen run --headless --frames N --seed S --json`.
8. Test altyapısı: özellik tabanlı testler, ECS için Miri, determinizm testleri, ilk kıyaslamalar.

**Çıkış ölçütü.**
- Örnek dünya 10.000 tick başsız çalışır; iki ayrı çalıştırmanın dünya hash'i aynıdır.
- 5.000. tick'te anlık görüntü → geri yükle → devam et, kesintisiz çalıştırmayla aynı hash'i verir.
- Her bileşenin JSON Schema'sı üretilir; JSON → dünya → JSON gidiş-dönüşü kayıpsızdır.
- Miri temiz; CI yeşil.
- Kıyaslama: 100 bin varlıkta basit bir sistemin tick süresi ölçülmüş ve kaydedilmiş (ADR-004 tetikleyicisi için).

**Model · efor.** İlk oturum (şema ve depolama yerleşiminin ayrıntılı tasarım notu): **Fable 5.2 · high**. Uygulama: **Opus 5.5 · high**. İskelet/CI gibi mekanik işler: Sonnet 5.5 · medium.

**İlerleme.** Ayrıntılı tasarım: [design/M1-cekirdek.md](design/M1-cekirdek.md).

| İş paketi | Durum |
|---|---|
| Tasarım notu | Tamamlandı (15 Kasım 2026) |
| 1. Çalışma alanı | Tamamlandı (15 Kasım 2026). CI dosyası yazıldı ama uzak depo olmadığı için henüz çalıştırılmadı |
| 2. `wish_core` | Tamamlandı (15 Kasım 2026) |
| 3. `wish_schema` | Tamamlandı (15 Kasım 2026): makrolar, JSON ⇄ bayt, JSON Schema, parmak izi. Toplam 60 test, biçim ve clippy temiz. Çıkış ölçütlerinden ikisi karşılandı: "her bileşenin JSON Schema'sı üretilir" ve "JSON gidiş-dönüşü kayıpsızdır" (her sonlu `f32`/`f64` için özellik testiyle) |
| 4. `wish_ecs` | Tamamlandı (15 Kasım 2026): depolama, türlü ve JSON erişim, sorgular, kaynaklar, metin tablosu. Model tabanlı test + Miri |
| 4b. `wish_ecs` ek | Tamamlandı (15 Kasım 2026): ertelenmiş komutlar, olaylar, hiyerarşi, değişiklik izleme. Model testi + Miri |
| 5. Zamanlayıcı | Tamamlandı (15 Kasım 2026): `wish_app` (sabit adım, sistem sırası, sistem başına rastgelelik akışı), `wish_core::Rng`. Aynı tohumla 600 tick boyunca aynı dünya |
| 6. Anlık görüntü / hash | Tamamlandı (15 Kasım 2026): dünya ve uygulama anlık görüntüsü, kararlı hash, `bounce` örneği. Çıkış ölçütleri: 10.000 tick × 2 aynı hash; 5.000'de anlık görüntü → geri yükle → devam = kesintisiz çalıştırma |
| 7. `wish_cli` | Tamamlandı (15 Kasım 2026): `wen run`, `wen schema`, `wen examples`; MCP ile aynı JSON zarfı |
| 8. Test altyapısı, kıyaslama | Tamamlandı (15 Kasım 2026): `wen bench` (100 bin varlıkta 0,50 ms/tick), bağımsız kapanış incelemesi ve bulgularının düzeltilmesi |

**M1 sonucu:** çıkış ölçütlerinin hepsi karşılandı. CI, depo özel GitHub deposuna gönderildikten sonra Windows ve Linux'ta yeşil. Ayrıntı ve inceleme bulguları: [design/M1-cekirdek.md](design/M1-cekirdek.md) §12, "WP8 ve M1 kapanışı".

## M2 — Veri katmanı

**Durum: tamamlandı (15 Kasım 2026).** Tasarım ve uygulama notları: [design/M2-veri-katmani.md](design/M2-veri-katmani.md). WP1–WP8: katı JSON + kanonik yazıcı, belge ve doğrulama, işlemler + geri al, prefab'lar, belge → dünya + `wen run --scene`, `AssetRef` + `wish_asset` + `wen mv`, `wen check/fmt/migrate/watch` + sıcak yükleme, M1 devirleri (`trybuild`, tek hata iletisi, `syn` 3) ve bağımsız kapanış incelemesi (9 bulgu, hepsi düzeltildi ve testlendi).

**M2 sonucu:** çıkış ölçütlerinin hepsi karşılandı; kanıt tablosu tasarım notunun §12 "M2 çıkış ölçütleri" bölümünde. Planlanandan sapmalar (gerekçeleri notta): dosya izleme `notify` yerine yoklamayla; `SceneId` yönetilen değil ayrılmış bileşen; içe aktarıcılar ve önbellek M3'e kaldı (planda da öyleydi).

**Çıktı.** `wish_scene` (sahne/prefab belge modeli, kanonik biçimlendirici, kimlik adresli yamalar, atomik işlemler, geri al/yinele, revizyonlar), `wish_asset` (varlık sunucusu, dosya izleme, önbellek, `*.meta.json`), `wen check`, `wen fmt`, `wen mv`, `wen migrate`.

**Çıkış ölçütü.**
- Yükle → kaydet bayt düzeyinde aynı dosyayı üretir.
- Kasıtlı bozulmuş 30 örnek dosyanın her biri konumlu, ipuçlu bir tanı üretir (çökme ya da anlamsız hata yok).
- Dosya diskte değişince dünya 1 saniyeden kısa sürede güncellenir; geçersiz dosya son geçerli durumu bozmaz.
- 100 rastgele işlem + 100 geri al = başlangıç dosyası.

## M3 — İlk piksel

**Durum: tamamlandı (15 Kasım 2026).** Tasarım ve uygulama notları: [design/M3-ilk-piksel.md](design/M3-ilk-piksel.md) (§14: WP1–WP8 ve "M3 kapanışı"). Bağımsız kapanış incelemesi 11 bulgu verdi; ikisi yüksek önemliydi (hostile glTF süreci çökertiyor / donduruyor) ve düzeltildi; kalan açıklar (örnekleme bellek kopyası → M6, aygıt kaybı kurtarma → M5) notta.

**M3 sonucu:** çıkış ölçütleri karşılandı; kanıt tablosu tasarım notunda. Dürüst eksikler: Khronos modellerinin "doğru görünmesi" yalnızca gözle doğrulandı (modeller depoda değil, CI karşılığı yok); pencere döngüsü CI'da çalışmaz ve penceredeki resmin içeriğine gözle bakılmadı. Sapmalar: pencere sunumu `softbuffer` ile (renderer ekran dışına çizer), WESL yerine kendi `#import` ön işlemcisi.

**Çıktı.** `wish_window`, `wish_render` (render grafiği, PBR, yönlü ışık + gölge, kamera, gökyüzü/ortam ışığı), glTF içe aktarma, başsız yakalama (`wen capture`), hata ayıklama görünümleri (kimlik, derinlik, normal, tel kafes).

**Çıkış ölçütü.**
- Standart glTF örnek modelleri doğru görünür (başvuru görüntüleriyle karşılaştırma).
- `wen capture` penceresiz çalışır; altın görüntü testi CI'da yazılım bağdaştırıcısıyla geçer.
- Kimlik tamponundan "şu pikselde hangi varlık var" doğru yanıtlanır.
- Gölgelendirici hatası ortak tanı formatında, dosya ve satırla raporlanır.

**İlk iş.** ADR-013 (WESL) doğrulaması.

## M4 — Kontrol düzlemi ve MCP

**Durum: tamamlandı (16 Kasım 2026).** Tasarım ve sonuç: [design/M4-kontrol-duzlemi.md](design/M4-kontrol-duzlemi.md) (§15 çıkış ölçütü kanıtları). 20 araç, MCP stdio sunucusu, komut satırı eşliği bayt bayt, ilk 10 değerlendirme görevi Sonnet 5.5 · medium ile **30/30** (iki koşu). **Plan değişikliği:** Streamable HTTP taşıması M8a'ya (editörle birlikte) kaydırıldı; gerekçe notun §1'inde.

**Çıktı.** `wish_control`, `wish_mcp` (stdio; HTTP M8a'da), tüm araçların komut satırı eşleri, `wen new` (proje iskeleti + `.mcp.json` + proje `CLAUDE.md`), MCP belge kaynakları, dünya denetçisi, `evals/` altında ilk 10 görev.

**Çıkış ölçütü.**
- Claude, boş bir projeden başlayıp **yalnızca MCP ile** bir sahne kurar, çalıştırır, görüntü alır ve sonucu doğrular — insan müdahalesi olmadan.
- Her MCP aracının komut satırı karşılığı aynı JSON'u üretir (otomatik eşlik testi).
- İlk 10 değerlendirme görevi Sonnet 5.5 · medium ile ≥ %80 başarı; ölçümler kaydedilir.
- Geçersiz her çağrı ipuçlu yapılandırılmış hata döndürür.

**T1 burada başlar:** kokpit maketleri ve etkileşim akışları (kod yok).

## M5 — Oyun mantığı

**Durum: tamamlandı (16 Kasım 2026)**, `m5-logic` dalından `main`'e birleştirildi. Notlar ve kanıtlar: [design/M5-oyun-mantigi.md](design/M5-oyun-mantigi.md) §17 (WP1–WP9) ve §18 "M5 sonucu". Bağımsız kapanış incelemesi 20 bulgu verdi (3 yüksek: olumsuz `expect` hep geçiyordu, derleme sırasında dosya/ortam okunabiliyordu, aynı adlı iki tür değerleri karıştırıyordu); hepsi düzeltildi. İki doğrulama kullanıcı adımı bekliyor: değerlendirme koşumu (kota) ve Windows Sandbox'ta temiz makine denemesi.

**Çıktı.** `wish_script` (wasmtime ev sahibi, ABI, yakıt/süre sınırı, tuzak yalıtımı), `wish_sdk` (konuk SDK'sı ve makrolar), kayıpsız sıcak yükleme, `wish_input` (eylem haritaları, enjeksiyon, kayıt/tekrar), `wish_test` (senaryo testleri), `script_build` ve `test_run` araçları.

**Çıkış ölçütü.**
- Claude, MCP + dosya düzenleme ile oynanabilir bir mini oyun yazar (hareket et, topla, kazan) ve senaryo testiyle kanıtlar.
- Kod değişikliği → çalışan oyun < 10 saniye; oyun durumu korunur.
- Sonsuz döngü içeren ve tuzak atan konuk kodu motoru dondurmaz/çökertmez; tanı üretir.
- Kaydedilen tekrar 10.000 tick boyunca hash sapması olmadan oynatılır.
- Sınır maliyeti kıyaslaması kaydedilmiş (ADR-011 tetikleyicisi).
- **Gömülü araç zinciri denemesi (ADR-023):** hiçbir geliştirme aracı kurulu olmayan temiz bir Windows'ta, paketlenmiş motor oyun kodunu derleyip çalıştırır.

## M6 — Oyun sistemleri

**Çıktı.** `wish_physics` (Rapier: katı cisimler, çarpıştırıcılar, ışın sorguları, karakter denetleyicisi), `wish_audio` (Kira + ses olay günlüğü), `wish_ui` (metin, görüntü, düğme, esnek yerleşim), iskelet animasyonu, nokta/spot ışıklar ve kümeleme, parçacıklar (temel), 2D alt kümesi (ortografik kamera, sprite).

**Çıkış ölçütü.**
- Her alt sistem kontrol düzleminden gözlemlenebilir ve başsız testlidir (ADR-017 kuralı).
- Fizikli sahne anlık görüntü/geri yükleme ve tekrar determinizm testlerini geçer.
- Değerlendirme takımı 25 göreve çıkar; Sonnet 5.5 · medium ≥ %85.

## M7 — Kendi motorumuzla oyun

**Çıktı.** Vitrin oyunu (kararlaştırıldı: küçük bir 3D platform oyunu), iki ek küçük örnek, 40+ görevlik değerlendirme takımı, sürtünme günlüğü ve düzeltmeleri, kısa motor kılavuzu (`wish://docs/guide`).

**Yöntem.** Oyunları bilinçli olarak orta ve küçük modellerle yaptırın. Ajanın takıldığı her yer bir motor hatasıdır: ya araç eksik, ya hata mesajı yetersiz, ya belge yanlış. Çözüm oyunda değil motorda yapılır.

**Çıkış ölçütü.**
- Vitrin oyunu baştan sona oynanabilir, senaryo testleri geçer.
- Değerlendirme takımı: Sonnet 5.5 · medium ≥ %90. Haiku 5.5 sonuçları kaydedilir.
- Claude dışında en az bir MCP istemcisiyle temel görevler doğrulanır.
- **Karşılaştırmalı ölçüm (kullanıcı kararı, 15 Kasım 2026):** değerlendirme takımının motordan bağımsız bir alt kümesi aynı modelle Unity ve Unreal'ın MCP bağlantılarında da koşulur; görev başarısı, adım sayısı, token ve hata sayısı yan yana raporlanır. Amaç "Claude ile daha sorunsuz" iddiasını ölçerek kanıtlamak; sonuç kötü çıkan alanlar sürtünme giderme listesine girer. Unity/Unreal kurulumu o sırada ayrıca onaya sunulur. (Bir ölçümdür, 1.0 özellik kapsamını büyütmez.)

## M8 — Hibrit arayüz

Arayüz hibrittir (ADR-018): elle düzenleme ve yapay zekâ kokpiti eşit düzeyde birinci sınıftır. İş ikiye bölünür; ikisi de `wish_editor` içindedir.

### M8a — Editör temeli (elle düzenleme)

**Ön koşul.** T1'in elle düzenleme maketleri tamamlanmış; ADR-018 arayüz teknolojisi kesinleşmiş. Yalnızca kontrol düzlemine (M4) ve render'a (M3) bağlıdır; bu yüzden M5–M7 ile **paralel** başlayabilir ve motorun kendi geliştirilmesinde de işe yarar.

**Çıktı.** Görünüm penceresi ve kamera gezinmesi; hiyerarşi; şemadan otomatik üretilen denetçi; taşıma/döndürme/ölçekleme tutamaçları; varlık tarayıcı (sürükle-bırak); materyal ve prefab düzenleme; girdi haritası düzenleyici; oynat/duraklat/adım at/geri sar denetimleri ve anlık görüntü zaman çizelgesi; konsol ve tanılar paneli; performans paneli; proje ayarları; "dış kod düzenleyicide aç"; Türkçe/İngilizce arayüz.

**Çıkış ölçütü.**
- **Hiçbir yapay zekâ bağlı değilken** bir kullanıcı, vitrin oyununun bir bölümünü yalnızca elle kurar, oynatır ve dışa aktarıma hazır hâle getirir.
- Eşlik denetimi: kontrol düzlemindeki her sahne/varlık işleminin arayüzde elle karşılığı vardır (otomatik denetim listesi).
- Arayüzdeki her eylem bir kontrol düzlemi işlemidir; geri al/yinele her eylemde çalışır.
- Wasm'da yeni tanımlanan bir bileşen, ek arayüz kodu yazılmadan denetçide düzenlenebilir.

### M8b — Yapay zekâ kokpiti

**Ön koşul.** M8a; T1'in kokpit maketleri; ADR-024 (Claude bağdaştırıcı yöntemi) kesinleşmiş.

**Çıktı.** Görünümle yan yana konuşma paneli, kokpit içi ajan ve yapay zekâ bağlantı ayarları, "göster ve söyle", etkinlik akışı ve kart başına geri al, otomatik denetim noktaları, önce/sonra karşılaştırma, yetki kipleri, canlı gözlem.

**Çıkış ölçütü.**
- Motoru hiç görmemiş bir kullanıcı, yalnızca konuşarak 30 dakikada oynanabilir bir mini oyun çıkarır (en az 3 kişiyle sınanır).
- Ajanın yaptığı her değişiklik etkinlik akışında görünür ve tek tıkla geri alınır.
- **Elden ele geçiş:** kullanıcı elle bir değişiklik yapar, ajan bunu görür ve üzerine devam eder; ajan çalışırken kullanıcı başka bir varlığı elle düzenler ve hiçbir değişiklik kaybolmaz.
- Elle ve ajanla yapılan değişiklikler aynı geçmişte, kimin yaptığı bilgisiyle görünür.
- Dış ajan (Claude Code) ve kokpit içi ajan aynı anda çalışırken çakışma veri kaybına yol açmaz.

## M9 — Ürünleştirme → 1.0

**Çıktı.** Tek paket kurulum (gömülü araç zinciriyle), `wen export` (bağımsız oyun paketi), ilk çalıştırma akışı (yapay zekâyı bağla ya da atla → şablon seç → ilk istek ya da elle başla), örnek projeler, kullanıcı belgeleri (TR/EN), güncelleyici (web sürümü), isteğe bağlı çökme raporu, üçüncü taraf lisans dosyası, son kullanıcı sözleşmesi (ücretsiz, komisyonsuz), Steam sayfası ve web sitesi.

**Çıkış ölçütü — "sorunsuz" çıtası.** [VISION.md](VISION.md) içindeki tüm başarı ölçütleri sağlanır. Ek olarak:
- Temiz Windows 10 ve 11 kurulumlarında: indir → ilk çalışan sahne < 5 dakika, ek kurulum yok.
- Dışa aktarılan oyun, motor kurulu olmayan bir makinede çalışır.
- Bilinen çökme hatası sıfır; açık "yüksek" öncelikli hata sıfır.
- Depo herkese açık yayımlanmış (MIT + Apache-2.0), katkı kılavuzu yazılmış; ad, marka ve alan adı uygunluğu doğrulanmış.

**Dış gereksinimler (para ve hesap gerektirir — sizin yapmanız gerekenler).** Steamworks hesabı ve Steam Direct başvuru ücreti; alan adı ve barındırma; web sürümü için kod imzalama sertifikası (yoksa Windows uyarı gösterir).

## M10 — 1.0 sonrası (sıra ihtiyaca göre)

- Gelişmiş render: GPU güdümlü çizim, bağsız kaynaklar, meshlet/sanal geometri, ışın izlemeli aydınlatma
- Web hedefi (WebGPU + Wasm)
- Araç zinciri gerektirmeyen ikinci betik dili
- Paralel sistem yürütme
- Çok oyunculu (geri almalı ağ kodu — determinizm altyapısı hazır)
- Linux ve macOS paketleri

---

## Risk kaydı

| Risk | Etki | Önlem | Denetim noktası |
|---|---|---|---|
| Kapsam büyümesi | Proje bitmez | 1.0 kapsamı sabit ([VISION.md](VISION.md)); yeni fikirler M10 listesine | Her kilometre taşı sonu |
| Gömülü araç zinciri sorunsuz paketlenemez | "Sıfır kurulum" sözü tutulamaz | M5'te temiz makinede deneme; yedek plan ADR-023 | M5 |
| Özel ECS performansı yetersiz | Büyük sahneler yavaş | Erken kıyaslama; yedek plan ADR-004 | M1 sonu |
| Wasm sınır maliyeti | Oyun mantığı yavaş | Toplu veri aktarımı; kıyaslama; ADR-011 tetikleyicisi | M5 |
| Determinizm sızıntısı | Tekrar/test güvenilmez | Her değişiklikte çift çalıştırma hash testi | Sürekli (CI) |
| Yapay zekânın yazdığı motor kodunda sessiz hata | Temel çürür | Çıkış ölçütleri kanıt ister; her kilometre taşı sonunda bağımsız kod incelemesi; `unsafe` için Miri | Her kilometre taşı sonu |
| Kokpit kullanışsız çıkar | Vizyonun kalbi zayıf kalır | T1: kod öncesi maket ve kullanıcı sınaması; M8b'de 3 kullanıcıyla sınama | T1, M8b |
| Hibrit arayüz kapsamı büyütür (tam elle düzenleme) | 1.0 gecikir | M8 ikiye bölündü; denetçi şemadan otomatik üretilir; uzman araçlar (animasyon, gölgelendirici grafiği, arazi) 1.0 dışında; M8a erken ve paralel başlar | M8a başı |
| Geliştirme makinesinde disk/bellek darlığı | Derleme başarısız/yavaş | Disk alanı açma, hedef klasörü yönlendirme | M1 öncesi |
| Tek kişi + ajan ekibi; inceleme yükü | Kalite düşer | Küçük adımlar, her adımda yeşil CI, kararların belgelere yazılması | Sürekli |

## Çalışma düzeni

1. **Her kilometre taşı kendi oturum(lar)ında.** Uzun tek oturum bağlamı bulandırır.
2. **Oturum başı:** `CLAUDE.md` + ilgili kilometre taşı bölümü + ilgili ADR'ler okunur; önerilen model · efor söylenir.
3. **Önce kısa tasarım notu** (`docs/design/Mx-konu.md`), sonra kod.
4. **Küçük adımlar:** her iş paketi derlenir, test edilir, commit edilir.
5. **Kanıtla bitir:** çıkış ölçütleri komut çıktısıyla gösterilir; "bitti" denmez, gösterilir.
6. **Karar değişirse** [DECISIONS.md](DECISIONS.md) güncellenir; kilometre taşı bitince bu dosyadaki durum işaretlenir.
7. **Kilometre taşı sonu:** bağımsız kod incelemesi (ayrı oturum, yüksek efor), sonra sonraki aşamaya geçiş.
