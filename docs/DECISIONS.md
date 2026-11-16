# Wish Engine — Mimari Kararlar (ADR)

Her karar: **ne**, **neden**, **reddedilen alternatifler**, **risk ve önlem**, **yeniden değerlendirme tetikleyicisi**.
Durum: **Kabul** (uygulanır) ya da **Geçici** (ilgili kilometre taşının başında doğrulanır).

Bir kararı değiştirmek serbesttir; sessizce çiğnemek değildir. Değişiklik bu dosyada yeni bir kayıtla yapılır.

## Özet

| # | Konu | Karar | Durum |
|---|---|---|---|
| 001 | Dil | Rust (stable) | Kabul |
| 002 | Kütüphane stratejisi | Mimari bizim, olgun kütüphaneler sarmalanarak kullanılır; Bevy/Godot üstüne kurulmaz | Kabul |
| 003 | Veri modeli | Şema önce: tek şemadan depolama, JSON, MCP şeması, Wasm ABI, editör | Kabul |
| 004 | ECS | Şema tanımlı depolamalı özel arketip ECS | Kabul |
| 005 | Zaman ve determinizm | Sabit adım, tohumlu rastgelelik, açık sistem sırası; garanti düzeyi D1 | Kabul |
| 006 | Anlık görüntü / tekrar | Dünya anlık görüntüsü, geri yükleme, metin tabanlı tekrar kaydı, tick başına hash | Kabul |
| 007 | Dosya formatları | Katı JSON + kanonik biçimlendirici + kimlik anahtarlı yapı + yol tabanlı başvuru | Kabul |
| 008 | Doğruluk kaynağı | Dosyalar; iki eşit düzenleme yolu (doğrudan dosya, MCP işlemi) | Kabul |
| 009 | Kontrol düzlemi | Tek iç API; MCP, komut satırı, editör onun yüzleri | Kabul |
| 010 | MCP araç tasarımı | Az sayıda yüksek kaldıraçlı araç, toplu işlem, belirteç bütçesi, yapılandırılmış hata | Kabul |
| 011 | Oyun mantığı | Wasm (wasmtime), durumsuz konuk modüller, ilk SDK Rust | Kabul |
| 012 | Render | wgpu, render grafiği, önce taban çizgisi, sonra katmanlı gelişmiş özellikler | Kabul |
| 013 | Gölgelendiriciler | WGSL + WESL modül sistemi | Geçici |
| 014 | Fizik | Rapier 3D, kendi bileşen API'mizin arkasında | Kabul |
| 015 | Ses | Kira + gözlemlenebilir ses olay günlüğü | Kabul |
| 016 | Girdi | Metin tanımlı eylem haritaları, enjeksiyon ve kayıt | Kabul |
| 017 | Test | Senaryo testleri, altın görüntüler, hash doğrulama, ajan değerlendirmeleri | Kabul |
| 018 | Arayüz | **Hibrit:** yapay zekâ kokpiti + tam elle düzenleme, ikisi de birinci sınıf; motor yapay zekâsız da eksiksiz çalışır | Kabul (arayüz teknolojisi Geçici) |
| 019 | Platform | Önce Windows masaüstü; web kısıtlarına baştan uyulur, web hedefi sonra | Kabul |
| 020 | Kapsam yönü | 3D öncelikli; 2D alt küme | Kabul |
| 021 | Uzlaşımlar | glTF koordinat sistemi, SI birimleri, İngilizce kod / Türkçe plan belgeleri | Kabul |
| 022 | Güvenlik | Proje klasörüne sınırlı yazma, yalnızca yerel ağ, Wasm kaynak sınırları | Kabul |
| 023 | Ürün ve dağıtım | Açık kaynak (MIT + Apache-2.0), ücretsiz, komisyonsuz; Steam + web; sıfır kurulum; oyun dışa aktarma | Kabul |
| 024 | Yapay zekâ bağlantısı | Kullanıcı kendi yapay zekâsını getirir; sağlayıcıdan bağımsız bağdaştırıcı, ilk sağlayıcı Claude | Kabul (yöntem Geçici) |
| 025 | Oyun kodunun derlenmesi ve örneklenmesi | cargo'suz tek `rustc` çağrısı + motorla gelen önceden derlenmiş SDK; ev sahibi bağlayıcısı yok; modül başına tick başına taze örnek | Kabul (M5 WP1 ölçümüyle) |

---

## ADR-001 — Dil: Rust

**Karar.** Motorun tamamı Rust (stable kanal, güncel edition). Sürüm `rust-toolchain.toml` ile sabitlenir.

**Neden.** Bellek güvenliği ve veri yarışı güvenliği derleme zamanında; derleyici hataları ajan için en kaliteli geri bildirim kaynağı ("derleniyorsa büyük olasılıkla doğru"); wgpu, wasmtime, rapier gibi ihtiyaç duyduğumuz her şeyin birinci sınıf ekosistemi Rust'ta; Claude Rust'ta güçlü.

**Alternatifler.** C++ (en geniş oyun ekosistemi, ama bellek hataları ajan döngüsünde sessiz ve pahalı), Zig (ekosistem olgun değil), C#/TypeScript (çöp toplayıcı, performans tavanı, determinizm zorlaşır).

**Risk ve önlem.** Derleme süreleri → küçük crate'ler, artımlı derleme, oyun mantığı Wasm'da olduğu için motor nadiren yeniden derlenir. `unsafe` kod → yalnızca `wish_ecs` ve `wish_script` içinde, Miri ile test edilir.

**Tetikleyici.** Yok (temel karar).

## ADR-002 — Mimari bizim, tekerlek yeniden icat edilmez

**Karar.** Motorun mimarisi ve değişmezleri bize aittir. Olgun, tek işi iyi yapan kütüphaneler kullanılır ve her biri kendi crate'imizin arkasına sarmalanır (değiştirilebilir kalsınlar diye):
`wgpu` (grafik), `winit` (pencere), `glam` (matematik), `rapier3d` (fizik), `kira` (ses), `wasmtime` (Wasm), `gltf` + `image` (içe aktarma), `rmcp` (MCP), `serde`/`serde_json`, `notify` (dosya izleme), `gilrs` (oyun kolu), `egui` (editör), `taffy` + `cosmic-text` (oyun içi arayüz).
Kesin sürümler M1 başında en güncel kararlı sürümlere sabitlenir.

**Neden.** Bir grafik API soyutlaması ya da fizik çözücü yazmak yıllar alır ve fark yaratmaz. Farkımız veri modeli, determinizm ve ajan arayüzünde.

**Alternatif: Bevy ya da Godot üstüne MCP katmanı.** İlk sonuca çok daha hızlı götürür. Reddedilme nedeni: şema önce, determinizm, anlık görüntü ve işlem değişmezleri dünya deposunun kendisinde sağlanmak zorunda; başkasının motoruna sonradan eklemek, kaçınmaya çalıştığımız "yama" sorununu tekrarlar. *Dürüst not: amaç yalnızca "yapay zekâyla hızlıca oyun yapmak" olsaydı doğru seçim Bevy/Godot olurdu. Amaç motorun kendisi olduğu için bu yolu seçiyoruz.*

**Risk ve önlem.** Kütüphane API değişimleri → sarmalayıcı crate'ler etkiyi tek noktada tutar.

## ADR-003 — Şema önce veri modeli

**Karar.** Her bileşen, kaynak ve olay türünün çalışma zamanı şeması (`TypeSchema`) vardır. Tek tanımdan şunlar türetilir:

1. Bellek yerleşimi (ECS sütunları)
2. JSON okuma/yazma (dosyalar ve MCP)
3. JSON Schema (MCP `schema_get`, editör doğrulaması)
4. Wasm ABI yerleşimi (konuk SDK bağlamaları)
5. Editör denetçi arayüzü
6. Belgeler

Rust'ta tür bir kez `#[component]` ile tanımlanır; alan belge yorumları (`///`), birim, aralık ve varsayılan değer şemaya taşınır. Ajan `schema_get` ile alanın anlamını, birimini ve geçerli aralığını görür.

**Neden.** Ajanın en sık hatası alan adı/tür/birim tahminidir. Tek kaynak, altı tüketici: eşitlenmesi gereken ikinci bir tanım yoktur.

**Alternatifler.** Ayrı bir arayüz tanım dili (iki kaynağı eşitleme yükü), yalnızca derleme zamanı yansıtma (Wasm'dan gelen türleri kapsamaz).

**Risk ve önlem.** Desteklenen alan türleri kısıtlıdır (skalerler, vektörler, dördeyler, renk, enum, sabit dizi, metin, liste, seçimlik, varlık başvurusu, varlık dosyası başvurusu). Bu kısıt bilinçlidir: anlık görüntü ve hash'i ucuz kılar.

## ADR-004 — Özel arketip ECS

**Karar.** Kendi arketip tabanlı ECS'imiz (`wish_ecs`): sütunlar şemadan türeyen bayt yerleşimleriyle saklanır. Özellikler: kararlı varlık kimlikleri (indeks + kuşak), deterministik yineleme sırası, ertelenmiş komutlar (sabit sırada uygulanır), kaynaklar, şemalı olaylar, değişiklik izleme, motorun yönettiği hiyerarşi (ebeveyn/çocuk).

**Neden.** Bileşenler çalışma zamanında (Wasm modülünden) tanımlanabilmeli; dünya tek seferde kopyalanıp hash'lenebilmeli; sıra deterministik olmalı. Bu üçü depolama katmanının özellikleridir.

**Alternatifler.** `bevy_ecs` (olgun ve hızlı, ama dinamik bileşenler ikinci sınıf, determinizm ve anlık görüntü garanti değil), `flecs` (C, yansıtması güçlü ama Rust bağlaması ve Wasm sınırı ek yük).

**Risk ve önlem.** Performans ve `unsafe` hataları → Miri + özellik tabanlı testler (proptest), erken kıyaslama; önce tek iş parçacıklı doğru sürüm, paralellik sonra.

**Tetikleyici.** M1 sonunda kıyaslamalar hedefin (100 bin varlık, 60 Hz) çok altındaysa `bevy_ecs` üstüne şema katmanı seçeneği yeniden değerlendirilir.

## ADR-005 — Zaman ve determinizm

**Karar.**
- Simülasyon sabit adımla ilerler (varsayılan 60 Hz); render ayrıdır ve ara değerleme yapar.
- Simülasyonda duvar saati yoktur. Tüm rastgelelik tohumlu `Rng` kaynağından gelir (sistem başına türetilmiş akışlar).
- Sistem sırası açık ve tamdır (kararlı topolojik sıralama, eşitlikte ada göre). Paralel çalıştırma yalnızca erişimleri ayrık sistemler için; komut tamponları sabit sırada birleştirilir.
- **Garanti düzeyi D1:** aynı derleme + aynı platform + aynı sahne/tohum/girdi → bit düzeyinde aynı dünya hash'i.
- **D2 (platformlar arası)** hedeftir, garanti değildir: Rapier `enhanced-determinism`, simülasyonda `libm` tabanlı matematik, Wasm'da NaN kanonikleştirme ile yaklaşılır.
- Render determinizm sınırının **dışındadır** (GPU sonuçları sürücüler arası bit düzeyinde aynı olmaz).

**Neden.** Hata yeniden üretimi, otomatik test, geri sarma ve ileride geri almalı ağ kodu — hepsi buna dayanır. Ajan için "aynı adımlar aynı sonucu verir" güveni vazgeçilmezdir.

**Risk ve önlem.** Determinizm sızıntıları (hash haritası yineleme sırası, iş parçacığı zamanlaması) → CI'da her değişiklikte "iki kez çalıştır, hash'leri karşılaştır" testi; sıralı olmayan koleksiyonlar simülasyon kodunda yasak (lint).

## ADR-006 — Anlık görüntü, geri yükleme, tekrar

**Karar.** Anlık görüntü = ECS sütunları + kaynaklar + fizik durumu + RNG + tick sayacı. Tekrar kaydı = başlangıç sahnesi + tohum + tick başına girdi akışı (metin, satır başına JSON). Her tick için dünya hash'i hesaplanabilir; tekrar oynatmada sapma ilk saptığı tick'te raporlanır. Bir oturum çatallanabilir (aynı andan iki farklı deneme).

**Neden.** Ajanın "geri al ve başka türlü dene" yapabilmesi; hataların dosya olarak paylaşılabilmesi; testlerin başlangıç durumunu ucuza kurabilmesi.

**Risk ve önlem.** Wasm konuk belleği anlık görüntüye girmez → kural: konuk modüller durumsuzdur, tüm durum dünyada yaşar (ADR-011). Motor bunu, modülü istediği an yeniden örnekleyerek zorlar.

## ADR-007 — Dosya formatları

**Karar.**
- Biçim: **katı JSON**. Diskteki gösterim ile MCP üzerinden giden gösterim aynıdır (tek gösterim).
- `wen fmt` kanonik biçimlendirici: kararlı anahtar sırası, kararlı sayı yazımı → temiz diff, bayt düzeyinde gidiş-dönüş.
- Yapılar **kimlik anahtarlıdır**, dizi değildir: bir sahnede varlıklar `"entities": { "player": {...} }` biçiminde. Yamalar indeksle değil kimlikle adreslenir (`/entities/player/components/Transform/translation`).
- Başvurular **proje köküne göre yoldur** (`"assets/models/crate.glb#Mesh0"`), GUID değildir. `wen mv` taşırken tüm başvuruları günceller.
- Uzantılar: `*.scene.json`, `*.prefab.json` (örnek = başvuru + geçersiz kılmalar), `*.mat.json`, `input.json`, `*.test.json`, proje bildirimi `wish.json`.
- İkili kaynak varlıklar standart formatlarda kalır (glTF/GLB, PNG/KTX2, OGG/WAV) ve yanlarında `*.meta.json` içe aktarma ayarı taşır.
- Her dosyada `"format": N` sürüm alanı; eski sürümler `wen migrate` ile yükseltilir.
- Büyük sahneler alt sahnelere bölünür.

**Neden JSON.** Dil modellerinin en güvenilir ürettiği yapılandırılmış biçim; her dilde araç desteği; JSON Schema ile doğrulama; MCP zaten JSON. RON Rust'a özgü ve modeller daha az görmüş; TOML derin hiyerarşide zayıf; YAML girinti hatalarına açık.

**Risk ve önlem.** Yorum satırı yok → varlık ve bileşenlerde isteğe bağlı `notes` alanı. Ayrıntı düzeyi → prefab + alt sahne ile dosyalar küçük tutulur; ajan tüm dosyayı değil sorguyla ilgili kısmı okur.

## ADR-008 — Dosyalar doğruluk kaynağıdır

**Karar.** Proje durumu diskteki dosyalardır. İki eşit düzenleme yolu vardır:
1. **Doğrudan dosya düzenleme** → dosya izleyici → doğrulama → sıcak yeniden yükleme. `wen check --json` derleyici gibi yapılandırılmış tanılar verir.
2. **MCP `edit_apply` işlemi** → bellekteki belge → kanonik dosya yazımı.

İkisi aynı kanonik çıktıyı üretir. Oynatma oturumları (simülasyon) sahne dosyalarına **asla** kendiliğinden yazmaz; kalıcı kılmak açık bir işlemdir.

**Neden.** Claude Code metin dosyası düzenlemede çok iyidir; bunu yasaklamak yerine güvenli kılıyoruz. Git ile sürümleme doğal çalışır.

**Risk ve önlem.** Eşzamanlı düzenleme (insan editörde, ajan MCP'de) → her belgenin revizyon numarası var; işlemler `base_revision` ile iyimser eşzamanlılık denetiminden geçer.

## ADR-009 — Tek kontrol düzlemi

**Karar.** `wish_control` taşımadan bağımsız tek API'dir (komutlar, sorgular, oturumlar). `wish_mcp`, `wish_cli` ve `wish_editor` onun ince istemcileridir. Kural: **kontrol düzleminde olmayan yetenek editörde de olamaz.** MCP taşımaları: stdio (Claude Code/Desktop başlatır) ve Streamable HTTP (çalışan editöre/oyuna bağlanma).

**Neden.** "Editörde var ama ajan yapamıyor" sınıfı sorunları yapısal olarak imkânsız kılar. MCP açık standart olduğu için diğer yapay zekâlar da aynı yüzeyi kullanır.

## ADR-010 — MCP araç tasarım ilkeleri

**Karar.**
- **Az ve güçlü araç** (~20–25). Çok araç, ajanın seçim doğruluğunu düşürür.
- **Toplu işlem:** tek çağrıda çok değişiklik, atomik.
- **Belirteç bütçesi:** varsayılan yanıtlar kısa; `detail` düzeyi ve sayfalama ile derinleşilir. Görseller varsayılan olarak küçültülür.
- **Yapılandırılmış hata:** `code`, `message`, `path`, `hint`, `candidates`, `docs`. Her hata "ne yanlış" ve "nasıl düzelir" der.
- **Her değiştiren çağrı** fark (diff), tanılar ve yeni revizyon döndürür; `dry_run` desteklenir.
- **Belgeler MCP kaynağı** olarak sunulur (`wish://docs/...`); ajan gerektiğinde çeker.
- **Adlandırma:** `ad_eylem` (`entity_query`, `sim_step`).
- Tasarım, sezgiyle değil **ajan değerlendirmeleriyle** (ADR-017) doğrulanır.

Taslak araç listesi: [MCP_API.md](MCP_API.md).

## ADR-011 — Oyun mantığı Wasm'da

**Karar.**
- Oyun mantığı WebAssembly modülleri olarak `wasmtime` içinde çalışır. İlk konuk SDK'sı Rust (`wish_sdk`).
- Çekirdek Wasm modülleri + sürümlenmiş, küçük bir ABI. (Component Model/WIT izlenir; tarayıcılar yerel desteklediğinde geçiş değerlendirilir.)
- **Konuklar durumsuzdur:** tüm oyun durumu ECS'te yaşar → kod değişince **kayıpsız sıcak yeniden yükleme**, anlık görüntüler eksiksiz.
- **Korumalı alan:** dosya, ağ, saat erişimi yok; yalnızca motorun sunduğu API. Yakıt/süre sınırı: sonsuz döngü motoru dondurmaz, tanı üretir. Tuzak (trap) → ilgili sistem devre dışı kalır, motor çalışmaya devam eder.
- Bileşen şemaları modülün içinde taşınır; yüklenince motora kaydolur.
- Sistemler eriştikleri bileşenleri bildirir; motor veriyi toplu (sütun dilimleri hâlinde) aktarır.
- Motoru genişleten, performans kritik eklentiler yerel Rust olabilir (sıcak yeniden yüklenmez).

**Neden.** Ajanın yazdığı kod güvenilmezdir: çökebilir, sonsuz döngüye girebilir. Wasm bunu izole eder. Motor yeniden derlenmeden saniyeler içinde yenileme sağlar. Wasm özünde deterministiktir. İleride başka diller eklenebilir.

**Alternatifler.** Yerel dinamik kütüphane sıcak yüklemesi (kırılgan, izolasyon yok), gömülü betik dili (Lua vb.: tip güvenliği zayıf, ajan hataları çalışma zamanına kalır).

**Risk ve önlem.** Ev sahibi–konuk sınır maliyeti → toplu veri aktarımı; M5'te kıyaslama. Rust konuk kodunun derleme süresi → küçük crate, artımlı derleme, hedef < 10 sn. Rust bilmeyen kullanıcı → kodu ağırlıklı olarak ajan yazar; ikinci konuk dili 1.0 sonrası.

**Tetikleyici.** M5 kıyaslamasında sınır maliyeti kare bütçesinin %20'sini aşarsa paylaşımlı bellek görünümleri tasarlanır.

**Ölçüm (M5 WP7, 16 Kasım 2026, `wen bench --script`):** aynı sistem 10.000 varlıkta yerel 0,014 ms, oyun kodu 1,87 ms → sınır **kare bütçesinin %11,2'si** (tetikleyicinin altında); tick başına sabit maliyet 0,04 ms. Ayrıntı ve iyileştirme notları: `docs/design/M5-oyun-mantigi.md` §17 WP7. **Kapanıştan sonra (M5 WP9):** geri yazma her değeri denetliyor (sonlu float, geçerli bool/enum) ve yalnızca gönderilenden farklı satırları yazıyor; aynı ölçüm ~240 ns/varlık, **%14** (tetikleyicinin altında).

## ADR-012 — Render

**Karar.**
- `wgpu` (Windows'ta DX12/Vulkan; aynı kod Metal ve WebGPU).
- **Render grafiği** mimarisi: geçişler bağımsız düğümler; yeni teknikler mevcut olanı bozmadan eklenir.
- **Önce taban çizgisi (1.0):** kümelenmiş forward+, glTF uyumlu PBR, basamaklı gölge haritaları, HDR + ton eşleme, ortam aydınlatması (IBL), MSAA, temel ardıl işlemler.
- **Gelişmiş katman (1.0 sonrası, yerel platformlar):** GPU güdümlü çizim (dolaylı çizim + hesaplama gölgelendiricisiyle ayıklama), bağsız (bindless) kaynaklar, meshlet/sanal geometri, ışın izlemeli aydınlatma. Hedef donanım (RTX sınıfı) bunu destekler.
- **İlk günden başsız (headless) render:** penceresiz, dokuya çizim; CI'da yazılım bağdaştırıcısı.
- **Ajan için hata ayıklama çıktıları birinci sınıf:** varlık kimliği tamponu, derinlik, normaller, tel kafes, çarpıştırıcılar. Kimlik tamponu "anlamsal görüş"ün (hangi varlık nerede, yüzde kaçı görünür) temelidir.

**Neden.** WebGPU geleceğin taşınabilir grafik standardı. Render grafiği "ileriyi gören" tercihleri sonradan eklemeyi mümkün kılar; taban çizgisi ise projeyi bitirilebilir kılar.

**Risk ve önlem.** Gelişmiş özellikler web'de yok → yetenek katmanları; taban çizgisi her yerde çalışır. Kapsam → gelişmiş katman 1.0'a bağlanmaz.

## ADR-013 — Gölgelendirici dili *(Geçici)*

**Karar.** WGSL; modül/içe aktarma için WESL. Materyaller veriyle tanımlanır (`*.mat.json`: gölgelendirici + parametreler); özel gölgelendiriciler metin dosyasıdır ve sıcak yüklenir. Derleme hataları ortak tanı formatına çevrilir.

**Tetikleyici.** M3 başında WESL araçlarının olgunluğu doğrulanır; yetersizse `naga_oil` ya da kendi basit içe aktarma ön işlemcimiz.

**M3 doğrulaması (15 Kasım 2026, M3 WP1).** `wesl` 0.6 ile deneme: içe aktarma çalışıyor ve çıktı `naga` için geçerli WGSL; içe aktarılan modüldeki sözdizimi hatası modül yolu + satır/sütunla geliyor. Ama (1) olmayan bir öğeyi içe aktarma hatası **konumsuz** ("in unknown module"), (2) `naga` doğrulama hataları birleştirilmiş çıktıdaki **dönüştürülmüş adlara** (`package_util_double`) işaret ediyor, (3) sürüm 1.0 öncesi (API değişiyor), (4) ağaç 83 paket. Değişmez 5 (her hata konumlu ve ipuçlu) bugün WESL ile sağlanamıyor.
**Karar (M3 için):** WGSL, `naga` ile ayrıştırma + doğrulama (konumlu `shader_syntax`/`shader_validation` tanıları, karakter sütunu). Motorun kendi gölgelendiricileri için modül birleştirme, bir `ShaderLibrary` arayüzünün arkasında küçük, kaynak haritalı bir içe aktarma ön işlemcisiyle (WP3); `naga` hatası kaynak haritasıyla özgün dosya/satıra geri çevrilir. WESL bu arayüzün arkasına sonra konabilecek biçimde tutulur. **Yeniden değerlendirme:** WESL 1.0'a ulaştığında ya da en geç kullanıcı gölgelendiricileri (`*.mat.json`, M6) geldiğinde. `naga_oil` denenmedi: Bevy'ye bağlı sürüm döngüsü ve kendi sözdizimi, ileride WESL'e geçişi zorlaştırır.

## ADR-014 — Fizik: Rapier

**Karar.** `rapier3d`, kendi bileşenlerimizin (`RigidBody`, `Collider`, …) arkasında. Sabit adımda çalışır; durumu serileştirilebilir (anlık görüntüye girer); `enhanced-determinism` açık.

**Alternatifler.** Jolt (çok güçlü, ama C++ bağlaması, derleme ve anlık görüntü karmaşıklığı), kendi fizik motorumuz (kapsam dışı).

**Tetikleyici.** Karakter denetleyicisi ya da araç fiziği ihtiyaçları Rapier'i aşarsa Jolt yeniden değerlendirilir; sarmalayıcı sayesinde oyun kodu etkilenmez.

## ADR-015 — Ses: Kira

**Karar.** `kira`. Ajan sesi duyamayacağı için **ses olay günlüğü** tutulur ("tick 412: `jump.ogg` çalındı, ses düzeyi 0.8, konum …") ve kontrol düzleminden sorgulanır. Ses çıkışı determinizm sınırının dışındadır; ses *olayları* içindedir.

## ADR-016 — Girdi: eylem haritaları

**Karar.** Oyun kodu ham aygıtı değil yalnızca **eylemleri** okur (`move`, `jump`). Eşlemeler `input.json` içinde. Kontrol düzlemi eylem enjekte edebilir, kaydedebilir, tekrar oynatabilir.

**Neden.** Ajanın oyunu gerçekten oynayabilmesi ve testlerin girdiyi betikleyebilmesi için tek temiz nokta.

## ADR-017 — Test ve doğrulama

**Karar.** Dört katman:
1. **Birim/özellik testleri** (Rust; ECS için Miri).
2. **Senaryo testleri** (`*.test.json`): sahne yükle → girdi ver → N tick ilerlet → dünya üzerinde ifade doğrula / görüntü karşılaştır.
3. **Determinizm testleri:** aynı tekrarı iki kez çalıştır, hash'leri karşılaştır; anlık görüntü → geri yükle → devam et = kesintisiz çalıştırma.
4. **Ajan değerlendirmeleri** (`evals/`): "zıplayan top yap", "üç platformlu bölüm kur" gibi görevleri bir ajan MCP üzerinden yapar; başarı oranı, araç çağrısı sayısı, belirteç maliyeti ölçülür. Bilinçli olarak **orta sınıf modelle** de koşulur: motor, zayıf modelle bile sorunsuz çalışıyorsa gerçekten ajan-dostudur.

Altın görüntü karşılaştırmaları algısal toleranslıdır ve CI'da sabit yazılım bağdaştırıcısıyla koşar.

**Kural.** Bir özellik; kontrol düzleminden gözlemlenebilir/denetlenebilir olmadan ve başsız testi yazılmadan **bitmiş sayılmaz**.

## ADR-018 — Arayüz: hibrit (yapay zekâ kokpiti + tam elle düzenleme)

**Karar — hibrit.** *(Kullanıcı kararı, 11 Kasım 2026.)* İki çalışma biçimi **eşit düzeyde birinci sınıftır** ve kullanıcı aralarında istediği an geçer:

- **Yapay zekâyla:** kokpit ögeleri (aşağıda).
- **Elle:** günümüz motorlarındaki gibi klasik editör araçları.

Hibritin dört kuralı:

1. **Yapay zekâsız tam kullanılabilirlik.** Hiçbir yapay zekâ bağlı değilken (hesap yok, internet yok, kota bitmiş) motor eksiksiz çalışır: sahne kurma, bileşen düzenleme, varlık içe aktarma, materyal, oynatma, hata ayıklama, test, dışa aktarma.
2. **Çift yönlü eşlik.** Ajanın yapabildiği her sahne/varlık işlemi elle de yapılabilir; elle yapılan her işlem de ajanın yapabileceği bir kontrol düzlemi işlemidir.
3. **Tek geçmiş.** Elle ve ajanla yapılan değişiklikler aynı işlem/revizyon/geri al yığınına ve aynı etkinlik akışına girer; her işlem kimin yaptığını (`actor`: insan ya da ajan adı) taşır.
4. **Elden ele geçiş.** Ajan, insanın elle yaptığı değişiklikleri görür ("az önce taşıdığım nesne" anlaşılır). Ajan çalışırken insan başka bir yeri düzenleyebilir; insanın dokunduğu varlıklar ajana bildirilir.

**Elle düzenleme araçları (1.0):** görünüm penceresi ve kamera gezinmesi; hiyerarşi; denetçi (şemadan **otomatik** üretilir: her bileşen, Wasm'da tanımlananlar dahil, ek kod yazmadan düzenlenebilir); taşıma/döndürme/ölçekleme tutamaçları; varlık tarayıcı (sürükle-bırak); materyal düzenleyici; prefab düzenleme; girdi haritası düzenleyici; oynat/duraklat/adım at/geri sar denetimleri ve anlık görüntü zaman çizelgesi; konsol ve tanılar paneli; performans paneli; proje ayarları.

**Oyun kodu** elle de yazılabilir: dış kod düzenleyiciyle (ör. VS Code) — motor dosyayı izler ve sıcak yükler. Motorun içine tam bir kod düzenleyici 1.0 kapsamında değildir ("dış düzenleyicide aç" + salt okunur görüntüleyici vardır).

**1.0 dışında kalan uzman araçlar:** animasyon zaman çizelgesi düzenleyicisi, görsel gölgelendirici grafiği, arazi şekillendirme, seviye boyama fırçaları. 1.0'a dek bunlar dış araçla (Blender), metin dosyasıyla ya da yapay zekâyla yapılır.

**Yapay zekâ kokpiti ögeleri.** Arayüz, klasik editöre sohbet penceresi eklenmiş hâli değildir; yapay zekâ ögeleri baştan tasarımın parçasıdır:

1. **Görünüm + konuşma** yan yana; istek yazılırken sahne görünür.
2. **"Göster ve söyle":** sahnede nesne, bölge ya da zaman aralığı seçimi isteğe yapılandırılmış bağlam olarak eklenir (varlık kimliği, ekran kutusu, tick aralığı).
3. **Etkinlik akışı:** ajanın her işlemi bir kart (fark + gerekçe + sonuç); kart başına geri al.
4. **Otomatik denetim noktaları:** her ajan görevinden önce anlık görüntü ve belge revizyonu işaretlenir.
5. **Önce/sonra karşılaştırma** ve kabul/ret.
6. **Yetki kipleri:** yalnızca öner · uygula-incele · tam özerk.
7. **Canlı gözlem:** ajanın oturumları, yakaladığı görüntüler, test sonuçları.

Tüm değişiklikler (elle ya da ajanla) kontrol düzleminden geçer; arayüzün özel yetkisi yoktur. İnsan ve ajan aynı anda düzenleyebilir (ADR-008 revizyonları). Arayüz dilleri: Türkçe ve İngilizce.

**Neden hibrit.**
- Küçük ve görsel işler (bir nesneyi biraz kaydırmak, renk seçmek) elle, tarif etmekten daha hızlıdır; büyük ve tekrarlı işler yapay zekâyla daha hızlıdır. İkisi birbirini tamamlar.
- Bugünkü modeller zevk ve ince görsel ayar gerektiren işlerde insana ihtiyaç duyar. Tek yol yapay zekâ olsaydı, modelin her hatası kullanıcıyı kilitlerdi.
- Yapay zekâ hesabı olmayan, çevrimdışı çalışan ya da kotası biten kullanıcı da motoru kullanabilmelidir. Denetimin kullanıcıda olması güven sağlar.
- İnsanın elle yaptığı düzeltme, ajan için en hızlı ve en net geri bildirimdir.
- Mimari bedeli düşüktür: tek kontrol düzlemi ve şema sayesinde elle araçlar aynı işlemlerin başka bir yüzüdür; denetçi şemadan otomatik üretilir.

**Reddedilen alternatif — tamamen yapay zekâ tabanlı, otomatik motor.** Daha küçük bir arayüz demektir, ama yukarıdaki nedenlerle bugün daha az işlevseldir. Modeller geliştikçe ağırlık kendiliğinden yapay zekâya kayar; hibrit tasarım buna engel değildir.

**Neden çekirdek önce.** Hem kokpit ögeleri hem elle araçlar, çekirdeğin zaten sağladığı değişmezlerin (işlem, geri al, anlık görüntü, fark, şema, anlamsal görüş) görsel yüzüdür — ek "sihir" gerekmez.

**Kapsam etkisi.** Bu karar **1.0 kapsamını büyütür**: tam elle düzenleme, arayüz işini yaklaşık iki katına çıkarır. Önlem: arayüz kilometre taşı ikiye bölündü (M8a editör temeli, M8b yapay zekâ kokpiti); uzman araçlar 1.0 dışında tutuldu; denetçi elle yazılmaz, şemadan üretilir.

**Geçici kısım — arayüz teknolojisi.** Varsayılan: süreç içi `egui` (görünüm aynı wgpu aygıtını paylaşır, tek paket dağıtımı kolay). Alternatif: HTTP kontrol düzlemine bağlanan web teknolojili arayüz (zengin metin/konuşma arayüzünde daha güçlü). M8 başında, kokpit tasarımı (yol haritasında T1) tamamlandıktan sonra kesinleşir.

**Risk ve önlem.** Kokpit tasarımı kod yazılmadan önce maketlerle sınanır (T1). Çekirdek arayüzden bağımsız olduğu için teknoloji değişse de motor etkilenmez.

## ADR-019 — Platform

**Karar.** Birincil hedef Windows masaüstü. Linux/macOS aynı koddan (CI'da derlenir). Web (WebGPU + Wasm) ayrı bir kilometre taşıdır, ama kısıtlarına **baştan** uyulur: taban render yolu WebGPU çekirdeğiyle sınırlı, iş parçacığı varsayımı zorunlu değil, konuk modüller çekirdek Wasm.

## ADR-020 — 3D öncelikli

**Karar.** Çekirdek 3D'dir. 2D, ortografik kamera + sprite (ve gerektiğinde 2D fizik) ile alt küme olarak 1.0'a doğru eklenir.

## ADR-021 — Uzlaşımlar

**Karar.**
- **Koordinat sistemi:** glTF ile aynı — sağ el, +Y yukarı, kamera −Z'ye bakar. İçe aktarmada dönüşüm gerekmez.
- **Birimler:** metre, saniye, kilogram. API'de açılar radyan. Dosyalarda belirsizlik olabilecek alanlar birimi adında ya da şemasında taşır (`fov_y_deg`, şemada `unit`).
- **Renk:** doğrusal iş akışı; renk dokuları sRGB.
- **Dil:** kod, tanımlayıcılar, API adları, tanı mesajları ve commit iletileri İngilizce. Planlama belgeleri Türkçe (motor yayımlanırsa İngilizceye çevrilir).
- **Ajan belgeleri:** `CLAUDE.md` asıl kaynaktır; diğer yapay zekâlar için `AGENTS.md` aynı içeriğe yönlendirir.

## ADR-022 — Güvenlik

**Karar.** MCP sunucusu yalnızca proje klasörüne yazar. HTTP taşıması yalnızca yerel makineye bağlanır ve oturum belirteci ister. `--read-only` kipi vardır. Wasm konukları yakıt, bellek ve süre sınırlıdır. Yıkıcı işlemler (dosya silme) geri alınabilir işlem olarak yürür.

## ADR-023 — Ürün ve dağıtım

**Karar.**
- Motor **açık kaynaktır**: MIT + Apache-2.0 ikili lisans (Rust ekosisteminin standardı). Depo ilk commit'ten itibaren bu lisans dosyalarını taşır. *(Kullanıcı kararı, 10 Kasım 2026.)*
- Motor **ücretsizdir**; motorla yapılan oyunlardan gelir payı, lisans ücreti ya da abonelik alınmaz. Seçilen lisanslar bunu zaten güvence altına alır: oyun geliştiricisi oyununu istediği lisansla, istediği fiyata satabilir.
- Dağıtım kanalları: **Steam** ve **kendi web sitemiz**. Güncelleme: Steam'de Steam'in kendi mekanizması; web sürümünde yerleşik güncelleyici.
- **Sıfır kurulum:** tek paket. Oyun kodunu derlemek için gereken araç zinciri (Wasm hedefli derleyici) motorla birlikte gelir ya da ilk açılışta motor tarafından sessizce kurulur. Kullanıcıdan Rust, Visual Studio ya da başka bir araç kurması **istenmez**.
- **Oyun dışa aktarma:** `wen export` (ve arayüzde tek tık) → çalışma zamanı + paketlenmiş varlıklar + Wasm modülleri içeren bağımsız oyun klasörü. Dışa aktarılan oyunda editör, MCP sunucusu ve derleyici bulunmaz.
- **Gizlilik:** varsayılan olarak hiçbir kullanım verisi toplanmaz. Çökme raporu yalnızca kullanıcı açıkça izin verirse gönderilir.
- **Üçüncü taraf lisansları:** seçilen tüm kütüphaneler (ADR-002) serbest lisanslıdır (MIT/Apache-2.0); ücretsiz dağıtıma ve ticari oyunlara engel yoktur. Atıf dosyası pakete otomatik eklenir; bağımlılık lisansları CI'da denetlenir.

**Neden.** Vizyonun doğrudan gereği. Steam'de ücretsiz bir motor yayımlamak mümkündür (başvuru başına tek seferlik bir Steam Direct ücreti vardır; kesin tutar ve koşullar yayım öncesinde Steamworks'ten doğrulanır).

**Risk ve önlem.**
- *En büyük ürün riski: gömülü derleme araç zinciri.* Rust → Wasm derlemesi için kullanıcı makinesinde bir derleyici gerekir; bunu sorunsuz paketlemek (boyut, antivirüs yanlış alarmları, ilk derleme süresi) kanıtlanmalıdır. → M5'te **temiz bir Windows kurulumunda** uçtan uca denenir.
- İmzasız çalıştırılabilir dosyalar Windows SmartScreen uyarısı verir → web sürümü için kod imzalama sertifikası (ücretli) gerekir; Steam sürümünde bu sorun yoktur.

**Tetikleyici.** M5 denemesinde gömülü araç zinciri "sorunsuz" çıtasını tutturamazsa: önceden derlenmiş SDK + daha hafif bir derleme yolu ya da araç zinciri gerektirmeyen ikinci bir konuk dili 1.0 kapsamına alınır.

## ADR-024 — Yapay zekâ bağlantısı: kullanıcı kendi yapay zekâsını getirir

**Karar.**
- Motor bir yapay zekâ modeli içermez ve yapay zekâ hizmeti satmaz. Kullanıcı kendi hesabını/aboneliğini bağlar.
- Bağlantı **isteğe bağlıdır**: motor yapay zekâ bağlı olmadan da eksiksiz çalışır (ADR-018).
- İki bağlantı yolu, ikisi de birinci sınıf:
  1. **Dış ajan:** Claude Code, Claude Desktop ya da MCP konuşan herhangi bir istemci, motorun MCP sunucusuna bağlanır. (M4'te hazır.)
  2. **Kokpit içi ajan:** arayüzdeki konuşma paneli, sağlayıcıdan bağımsız bir **ajan bağdaştırıcısı** arayüzü üzerinden çalışır. İlk bağdaştırıcı Claude'dur. (M8.)
- Kimlik bilgileri motorun kendi dosyalarında düz metin olarak saklanmaz; işletim sisteminin kimlik deposu ya da sağlayıcının kendi oturumu kullanılır.
- Kokpit, hangi yoldan gelirse gelsin tüm ajan etkinliğini aynı etkinlik akışında gösterir (dış ajan da MCP üzerinden aynı işlemleri üretir).

**Neden.** "Komisyon yok" ilkesi yapay zekâ kullanımını da kapsar. Sağlayıcıdan bağımsızlık, motorun tek bir şirkete bağımlı kalmamasını sağlar; Claude önceliği, ilk ve en iyi sınanan bağdaştırıcı olmasıyla korunur.

**Geçici kısım.** Kokpit içi Claude bağdaştırıcısının kesin yöntemi (Claude Code'u alt süreç olarak sürmek, Claude Agent SDK ya da doğrudan API) M8 başında, o günün güncel belgelerine göre seçilir.

## ADR-025 — Oyun kodunun derlenmesi ve örneklenmesi

**Karar.**
- **Derleme yolu C:** oyun kodu (`scripts/`) **cargo olmadan, tek bir `rustc` çağrısıyla** `wasm32-unknown-unknown` için derlenir. `wish_sdk` (wasm32 rlib) ve `wish_sdk_macros` (ev sahibi proc-macro kütüphanesi) motorun kendi derlemesinde üretilir ve motorla birlikte gelir; oyun derlemesi onları `--extern` ile kullanır. Bağlama `rust-lld` ile yapılır (rustc'yle gelir). Kullanıcının makinesinde **ev sahibi bağlayıcısı (MSVC `link.exe`) hiç gerekmez.**
- **Gömülü araç zinciri** = sabit rustc'nin asgari kopyası: `rustc` + `rustc_driver`, `rust-lld` (+ `gcc-ld/wasm-ld`), `rust-std-wasm32-unknown-unknown`, önceden derlenmiş SDK. Cargo, rustup, Visual Studio gerekmez.
- **Örnekleme:** modül başına **tick başına bir taze örnek**; o tick'teki sistem çağrıları onu paylaşır, bir çağrı başarısız olursa (tuzak, yakıt, bellek) örnek atılır ve sıradaki çağrı yenisini alır. Durum tick'ten tick'e asla taşınmaz: anlık görüntüler tick sınırında alındığı için eksiksizdir, sıcak yükleme tick sınırında kayıpsızdır.
- wasmtime havuz ayırıcısıyla (64 yuva) çalışır.

**Neden (M5 WP1 ölçümleri, 16 Kasım 2026, geliştirme makinesi, Windows 10, release; `wish_script/tests/build_paths.rs` ve `rust_guest.rs` ile yeniden üretilir):**
- *Yol A (cargo + proc-macro):* bağlayıcı ortam değişkeniyle bozulunca derleme **başarısız** — proc-macro ev sahibi için bağlanıyor; temiz Windows'ta çalışmaz.
- *Yol C:* `env_clear()` ile boş ortamda (yalnızca kopyalanmış araç zinciri `PATH`'te, rustup/VS değişkenleri yok) derlendi ve modül çalıştı; `--print link-args` bağlayıcının `rust-lld` olduğunu gösterdi. Süre: ilk derleme 1,95 sn (yeni kopyalanmış 200 MB'lık `rustc_driver` dll'inin ilk yüklenmesi dahil), **tek satır değişiklikten sonra 0,18 sn**, değişiklik yokken 0,13 sn. Hedef < 10 sn rahatça tutuyor.
- *Araç zinciri boyutu:* açılmış 437 MiB (`rustc_driver` 201 MiB, `rust-lld` 117 MiB, wasm32 std 100 MiB), gzip ile 141 MiB. Unity/Unreal kurulumlarının yanında küçük; ADR-023 tetikleyicisi tetiklenmedi. (Sıkıştırma `zstd`/`xz` ile daha da küçülür; gereksiz `gcc-ld` sarmalayıcıları atılabilir — WP7.)
- *Taze örnek maliyeti:* depo 0,4 µs + örnekleme 6,4 µs + dışa aktarma/ayırma 2,3 µs + bırakma 7,2 µs ≈ 16–28 µs (ölçüme göre değişken). Çağrı başına taze örnek, 20 sistemlik bir oyunda tick başına ~0,5 ms ederdi; tick başına örnekle ~20 µs + sistem başına birkaç µs. Havuz ayırıcı istek üzerine ayırmaya göre ~%20 hızlı.
- *Sınır maliyeti:* konuğa 24 baytlık iki sütun (konum + hız) kopyalayıp geri almak dahil varlık başına ~13–15 ns (yerel döngü ~1,5 ns). 10.000 varlıkta çağrı başına ~135 µs: kare bütçesinin %0,8'i. ADR-011 tetikleyicisi (%20 = 3,3 ms) çok uzakta.

**Alternatifler.** Yol A (cargo + proc-macro): temiz makinede bağlayıcı yok — elendi. Yol B (proc-macro'suz, `macro_rules!` bildirimleri): çalışır ama Rust'ın gerçek öznitelik sözdizimini ve iyi derleyici hatalarını kaybettirir; C aynı bedeli ödemeden aynı sonucu verir. Çağrı başına taze örnek: en katı durumsuzluk, ama tick başına örneğe göre ~10 kat pahalı ve kazandırdığı bir garanti yok (anlık görüntü zaten tick sınırında).

**Bedel.** İlk sürümde oyun kodu üçüncü taraf crate kullanamaz (yalnızca `wish_sdk`); önceden derlenmiş SDK sabit rustc sürümüne bağlıdır (`rust-toolchain.toml` değişince yeniden üretilir; uyuşmazlık `script_toolchain_mismatch` ile açıklanır). Bir tick içinde iki sistem bir `static` üzerinden konuşabilir: belirlenimcidir ama SDK bunu önermez ve garanti etmez.

**Tetikleyici.** Değerlendirmelerde ajanların üçüncü taraf crate ihtiyacı tekrar tekrar görülürse yol B'ye benzer bir cargo yan yolu (bağlayıcı gerektirmeyen crate'lerle) eklenir. Tick başına örnek maliyeti kıyaslamada (WP7) bütçenin %5'ini aşarsa örneği tick'ler arasında yeniden kullanıp belleği sıfırlama yolu denenir.

---

## Kullanıcı kararları

| Konu | Karar | Tarih |
|---|---|---|
| Açık kaynak mı? | **Evet** — MIT + Apache-2.0 (ADR-023) | 10 Kasım 2026 |
| İlk vitrin oyunu (M7) | **Küçük bir 3D platform oyunu** — fizik, kamera, girdi, ses ve arayüzü birlikte sınar | 10 Kasım 2026 |
| Ad | **Wish Engine** — "dile, olsun"; İngilizce, kısa, akılda kalıcı | 10 Kasım 2026 |

**Adlandırma ayrıntıları.**
- Crate öneki `wish_`, belge adresleri `wish://…`, proje bildirimi `wish.json`.
- Komut satırı aracı `wen`dir (**W**ish **En**gine), `wish` **değil**: `wish` adı Tcl/Tk'nin kendi komutudur ve macOS ile Git for Windows'ta hazır gelir; aynı adı kullanmak çakışırdı.
- 10 Kasım 2026'da yapılan hızlı web aramasında bu adı taşıyan bir oyun motoru bulunmadı. Bu kesin bir marka denetimi değildir; Steam'de yayım öncesinde marka ve alan adı uygunluğu ayrıca doğrulanır (M9).
