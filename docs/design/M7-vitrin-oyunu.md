# M7 — Kendi motorumuzla oyun

Durum: tasarım (16 Kasım 2026). Dal: `m7-game`. İlgili kararlar: ADR-016 (girdi), ADR-017 (test), ADR-020 (3D öncelikli), "İlk vitrin oyunu: küçük bir 3D platform oyunu" (DECISIONS, 10 Kasım 2026).

## 1. Hedef ve yöntem

M7 motoru **kullanarak** sınar: bir vitrin oyunu, iki küçük örnek, 40+ görevlik değerlendirme takımı, sürtünme günlüğü ve düzeltmeleri, kısa motor kılavuzu.

**Yöntem (kullanıcı kararı, 16 Kasım 2026):** vitrin oyununu bu çalışma oturumundaki ajan (Opus 5.5) **yalnızca motorun kendi araçlarıyla** kurar — sahne değişiklikleri `edit_apply` ile, gözlem `capture_frame`/`describe_view`/`entity_query` ile, oyun kodu dosya düzenleme + `script_build` ile, kanıt senaryo testleriyle — tıpkı bir dış ajanın yapacağı gibi. Sahne dosyası elle yazılmaz. Takılınan her yer **sürtünme günlüğüne** (`docs/friction.md`) girer ve oyunda değil **motorda** düzeltilir (eksik araç, yetersiz hata iletisi, yanlış belge, hata). Sonnet 5.5 · medium ve Haiku 5.5 ile gerçek ölçüm (değerlendirme takımı + "oyun yap" görevleri) M7'nin sonunda, kullanıcı onayıyla tek seferde koşulur (kota).

**Çıkış ölçütleri (ROADMAP):**
1. Vitrin oyunu baştan sona oynanabilir, senaryo testleri geçer.
2. Değerlendirme takımı: Sonnet 5.5 · medium ≥ %90; Haiku 5.5 sonuçları kaydedilir. (Koşum: sonda, onayla.)
3. Claude dışında en az bir MCP istemcisiyle temel görevler doğrulanır. (İstemci kurulumu onaya sunulur.)
4. Karşılaştırmalı ölçüm: motordan bağımsız bir alt küme Unity ve Unreal'ın MCP bağlantılarında da koşulur. (Kurulum onaya sunulur; ölçümdür, 1.0 kapsamını büyütmez.)

## 2. Vitrin oyunu: "Ada Atlama" (çalışma adı)

Kısa bir 3D platform oyunu, 2–4 dakikalık; fizik, kamera, girdi, ses, arayüz, parçacık ve animasyonu birlikte sınar.

- **Oyuncu:** karakter denetleyicili (M6), koşma, zıplama, havada az kontrol; düşünce son denetim noktasından yeniden başlar.
- **Kamera:** oyuncuyu arkadan ve yukarıdan izleyen, yumuşak takip eden üçüncü şahıs kamera (oyun kodu).
- **Bölüm:** gökyüzünde adacıklar; sabit platformlar, **hareketli platformlar** (kinematik gövde, oyun kodu sürer; karakteri taşımalı), düşen/kaybolan platform, tehlike (dikenler ya da lav → yeniden başlama), denetim noktaları (bayrak), bitiş kapısı.
- **Toplananlar:** kristaller (sensör), sayaç arayüzde; hepsi + bitiş = kazanma ekranı (süre ve kristal sayısı).
- **Ses:** zıplama, toplama, düşme, müzik (kodla üretilmiş kısa WAV'lar; indirme yok).
- **Görsel:** ilkel şekiller + malzemeler + nokta ışıklar; toplamada parçacık; bitişte parçacık; gerekirse kodla üretilen basit glTF.
- **Kanıt:** senaryo testleri (bölüm bitirilebilir, düşünce denetim noktasına dönülür, hareketli platform taşır, arayüz doğru sayar), kaydedilmiş bir tam oynanış tekrarı (CI'da bit düzeyinde), altın görüntü.

Varlık üretimi indirme gerektirmez; ileride gerçek modeller/sesler istenirse kullanıcıya sorulur.

## 3. Sürtünme günlüğü

`docs/friction.md`: her satır bir takılma. Alanlar: **no**, **ne oldu** (çağrı, ileti, beklenen), **tür** (eksik araç / yetersiz ileti / yanlış ya da eksik belge / hata / API boşluğu / performans), **etkisi** (kaç adım ya da deneme kaybettirdi), **düzeltme** (commit), **durum**. Kural: aynı takılma ikinci kez yaşanmamalı; her düzeltme ya bir test ya bir değerlendirme görevi ile korunur. Günlük, M7 sonunda Sonnet/Haiku koşumlarının sonuçlarıyla birleşir.

## 4. Beklenen motor işi (ölçülerek karar verilir)

Önceden bilinen adaylar; gerçekten gerekirse yapılır, gerekmezse günlüğe "gerekmedi" yazılır:
- Hareketli platformun karakteri taşıması (Rapier karakter denetleyicisi + kinematik platform hızı).
- Karakter çarpıştırıcısının sorgularda aynı tick'te güncellenmesi (M6 bulgu 8).
- İç içe prefab (M2'de ölçülmek üzere bırakıldı) — adacık/platform kümeleri için.
- Basamaklı gölge (CSM) — açık gökyüzü sahnesinde gölge kalitesi (M3'te ölçülmek üzere bırakıldı).
- Oyun kodu yardımcıları (ör. yumuşak takip için `lerp`/`look_at`), kılavuzda platform oyunu tarifi.
- `edit_apply`'ın canlı oturuma uygulanması (M8a'ya devredildi; vitrin oyununda gerekirse öne alınması değerlendirilir).

## 5. İki küçük örnek

Vitrin oyunundan farklı alanları sınar: (a) **2D tuğla kırma** (ortografik kamera, sprite, 2D fizik kilitleri, arayüz, ses); (b) **arayüz ağırlıklı küçük bir oyun** (menü, düğmeler, metin; ör. bir bilgi yarışması ya da zamanlayıcılı tıklama oyunu). İkisi de testli, CI'da kaydedilip tekrar oynatılır.

## 6. Değerlendirme takımı 40+

Görevler 26–40+: platform oyunu yapı taşları (takip kamerası, hareketli platform, denetim noktası, kazanma ekranı, tehlike), sürtünme günlüğünden çıkan durumlar, iki örneğe benzer küçük oyunlar. Hepsi `selftest` ile kanıtlanır. Ölçüm: sonda Sonnet 5.5 · medium (hedef ≥ %90) ve Haiku 5.5 (kayıt).

## 7. Başka MCP istemcisi ve karşılaştırma

- **Başka istemci:** adaylar MCP Inspector (resmî, Node gerekir), VS Code / Cursor ajan modu. Kurulum o iş paketinde kullanıcıya sorulur.
- **Unity/Unreal karşılaştırması:** motordan bağımsız görevler (küp koy, ışığı ayarla, nesneyi görünür yap, fizikli düşen kutu…) aynı modelle üç motorda; başarı, adım, token, hata sayısı yan yana. Unity/Unreal kurulumu ve MCP bağlantıları o iş paketinde kullanıcı onayına sunulur.

## 8. İş paketleri ve önerilen model

| WP | İş | Model · efor |
|---|---|---|
| WP1 | Tasarım notu, sürtünme günlüğü, vitrin oyunu projesi (`examples/islands`) ve ilk sahne: oyuncu, takip kamerası, birkaç platform | Opus 5.5 · high |
| WP2 | Oynanış: kristaller ve arayüz, tehlike + denetim noktaları, ses, parçacık; sürtünme düzeltmeleri | Opus 5.5 · high |
| WP3 | Hareketli/düşen platformlar, bitiş ve kazanma ekranı, tam bölüm, senaryo testleri, kaydedilmiş oynanış; motor düzeltmeleri | Opus 5.5 · high |
| WP4 | Motor kılavuzu (`wish://docs/guide`) ve SDK sayfasının oyun yapımı tarifleriyle gözden geçirilmesi | Sonnet 5.5 · high |
| WP5 | İki küçük örnek | Sonnet 5.5 · medium |
| WP6 | Değerlendirme görevleri 26–40+ (`selftest`) | Sonnet 5.5 · high |
| WP7 | Başka MCP istemcisi; Unity/Unreal karşılaştırma düzeneği (kurulumlar onayla) | Opus 5.5 · high |
| WP8 | Bağımsız kapanış incelemesi, ölçüm koşumları (onayla), M7 sonucu, `main`'e birleştirme | Opus 5.5 · high |

## 9. Riskler

| Risk | Önlem |
|---|---|
| Kurucu ajan (Opus) motoru "fazla iyi" bildiği için sürtünmeyi kaçırır | Yalnızca araçlarla çalışma kuralı; her başarısız çağrı ve her "kaynağa bakmam gerekti" anı günlüğe; sonda Sonnet/Haiku ölçümüyle doğrulanır |
| Hareketli platformlar karakter denetleyicisiyle sorunlu | Erken (WP1–WP2) denenir; gerekirse motor tarafında platform hızı karaktere aktarılır |
| Kapsam büyür (vitrin oyunu "bitmez") | Bölüm 2–4 dakika; yeni mekanik ancak motor eksiğini ortaya çıkarıyorsa |

## 10. Uygulama notları

### WP1–WP3 — vitrin oyunu "Ada Atlama" (`examples/islands`)

- **Nasıl kuruldu:** yalnızca araçlarla, `wen do` ile (bir süreçte birden çok çağrı; oturumlar süreç boyunca yaşar). Sahne `edit_apply`, sesler `asset_create`, kod `scripts/src/lib.rs` + `script_build`, gözlem `capture_frame`/`describe_view`/`entity_get`, kanıt `test_run`. Sahne dosyasına elle dokunulmadı.
- **Oyun:** başlangıç adası → iki basamak ada → kinematik asansör (oyun kodu zamanla sürer: bekle, yumuşak git, bekle, dön) → bitiş adası ve kapı. Altı kristal (sensör; ses + parıltı), denetim noktası bayrağı (yeniden doğma yeri), dünyadan düşünce denetim noktasına dönüş, takip kamerası (zamandan bağımsız yumuşatma), arayüzde kristal sayacı, rüzgâr sesi, kazanma ekranı (kristal sayısı + süre).
- **Kanıt:** 4 senaryo testi (`first_jump`, `checkpoint`, `win` — bölümün tamamı, altı kristal, kazanma ekranının altın görüntüsü) ve CI testi `the_islands_showcase_is_won_by_its_tests_and_replays` (derleme, denetim, testler, kaydedilen oynanışın bit düzeyinde tekrarı).
- **Sürtünme günlüğü F1–F8** (hepsi motorda düzeltildi; F8 tasarımda beklenen işti): `wen do` okunaklı JSON; proje kılavuzunda oyun sistemleri; oturum açıkken sahne okuyan araçlara not; yürüyen karakterde temas olayı titremesi; **yeni araç `asset_create`**; SDK `asset()`; **`entity_get` kaynakları okur**; **hareketli platform karakteri kesin taşır** (Rapier'in yarım takibi devre dışı, platformun gerçek yer değiştirmesi; 3 sn'de 6,5 m'de birebir).
- **Kendi hatalarım (motor değil):** test beklentisinde kamera konumu; rota yönleri; yeniden doğma noktasının bir kristalle çakışması; kristal yolunun bitiş sensörüne değmesi. Bunlar oyun tasarımı/oynama hatalarıydı; araçların hata iletileri (adım, tick, okunan değerler) her birini tek bakışta gösterdi — iyi işaret.

### WP4 — tarifler ve kılavuz

- **`wish://docs/recipes`** (`crates/wish_control/src/recipes.md`): sekiz çalışan parça — platform karakteri, takip kamerası, toplanabilirler + puan yazısı + ses + parıltı, düşme ve denetim noktaları, hareketli platform, kazanma ekranı ve başla düğmesi, 2D sahne, senaryo testleri ve tekrarlar. Her biri tam bir `scripts/src/lib.rs` ve sahne işlemleri; kod vitrin oyununda kanıtlanmış parçalardan.
- **Tarifler çürümez:** `wish_control/tests/recipes.rs` her tarifin kodunu boş bir projede derler (uyarısız), işlemlerini uygular, projeyi denetler ve bir oturum koşar; test örneklerinin biçimini sınar. İlk koşuda 2D tarifindeki gerçek bir hatayı yakaladı (yeni projede `ground` zaten var).
- **Bulunabilirlik:** MCP kaynak listesinde, `docs_search`'te (bölüm bölüm), kılavuzda kısa "Game systems" bölümüyle, yeni projenin `CLAUDE.md`'sinde ve MCP sunucu yönergelerinde; başka motorların adları (`FollowCamera`, `MovingPlatform`, `Checkpoint`, `Cinemachine`…) kavram ipucuyla tariflere yönlendirir.

### WP5 — iki küçük örnek

- **`examples/bricks` (2D tuğla kırma):** ortografik kamera, sprite'lar, 2D fizik kilitleri (Z ötelemesi, tüm dönmeler), `ccd`'li sekme katsayısı 1 olan top, sabit hızlı top ve raketin çarpma yerine göre yön vermesi (oyun kodu), puan/can arayüzü, sesler (`asset_create`), kazanma/kaybetme paneli. Testler: açılış, ve **otomatik pilotla tam oyun** (40 tuğla, ~5.000 tick). İlk denemede otomatik pilot sonsuz döngüye girdi (düz raket topu hep aynı açıyla yansıtır) → klasik çözüm; bu oyun tasarımıydı, motor değil.
- **`examples/quiz` (arayüz ağırlıklı):** menü → beş soru × dört şık düğmesi → sonuç; her şey `UiNode`; oyun kodu yalnızca `ButtonClicked`'e tepki verir ve metin yazar. Testler işaretçiyle tıklayarak oynar (tam doğru 5/5 ve bir yanlış 4/5).
- **CI:** her ikisi de derlenir, testleri koşar, kaydedilen koşu bit düzeyinde tekrar oynatılır (`script_tools.rs`).
- **Sürtünme F9–F10 (düzeltildi):** arayüz çocuklarının sırası belgesizdi (`order`); geçersiz UTF-8 dosyada ipucu kodlamayı söylemiyordu.

### WP6 — değerlendirme görevleri 26–41

- 16 yeni görev (`crates/wish_evals/src/m7_tasks.rs`); toplam **41**; **`selftest` 41/41** (her görev referans çözümden önce başarısız, sonra başarılı). Hepsi motorun yaptığına bakar (oturum koşusu, `describe_view`, günlükler, gizli senaryo testleri).
- **Oyun kodu (6):** takip kamerası, hareketli platform, düşünce yeniden doğma, sensörle mücevher toplama + `Score`, itmeyle (`BodyImpulse`) zıplama, saniye sayacı yazısı (`text()`).
- **Sahne/araç (10):** `asset_create` ile zil sesi (perde ve uzunluk ölçülür) ve halka resmi + arayüzde gösterme, çalışan oyunun kaynağını okuyup nota yazma (`entity_get resources`), dikey menü (arayüz sırası `order`), 2B sekme (kilitler), sahne ışığı konisi, animasyonu bir kez oynatma, çarpışma katmanı/maskesiyle "hayalet" top, ikinci seviye sahnesi, tekrar kaydı.
- Çoğu sürtünme günlüğünden gelir: ajanın takılabileceği yerler (F5 asset_create, F7 kaynaklar, F9 sıra…). **Gerçek koşum (Sonnet · medium hedef ≥ %90, Haiku kayıt) kota harcar → M7 sonunda kullanıcı onayıyla.**

### WP7 — başka MCP istemcisi ve karşılaştırma düzeneği

- **Başka istemci (çıkış ölçütü 3): MCP Inspector 2.13.0** (resmî, kullanıcı onayıyla `C:\wish-build\mcp-inspector`'a kuruldu, iş bitince silindi), CLI kipi; her adım taze bir `wen mcp` süreci (el sıkışma her seferinde). `tools/mcp_inspector_check.py`: araç listesi `--strict` şema taşınabilirlik denetimiyle (25 araç, sorun yok), kaynaklar + kılavuz okuma, istemler, `project_info`, `schema_get`, `edit_apply` + `entity_get`, hata kodu + ipucu, `describe_view` + `capture_frame` (PNG), `script_build`, `test_run` (düşen kutu + oyun kodu sayacı), `asset_create` + `check` → **11/11** (`evals/results/m7-mcp-inspector.md`). Motorda değişiklik gerekmedi.
- **Karşılaştırma düzeneği (çıkış ölçütü 4):** `tools/compare/` — 8 motordan bağımsız sahne görevi (küp koy, boya, düşen kutu, nokta ışığı, kamerayı hedefe çevir, ebeveyn değiştir, sil, ölçekle), iki motorda aynı metin ve aynı başlangıç sahnesi (Unity'de Z aynalı; `unity/CompareSetup.cs`). Wish denetimi motorun araçlarıyla; Unity denetimi kaydedilen `.unity` dosyasını okuyarak (kurulum gerektirmeyen küçük YAML okuyucu). Koşucu başarı, tur, araç çağrısı/hatası, token ve maliyeti toplar. `selftest` ücretsiz ve geçiyor (Wish: 8 görev öncesi başarısız/sonrası başarılı; Unity: okuyucu örnek dosyada). Gerçek koşum Unity projesinden önce okuyucunun gerçek başlangıç sahnesinde her görevi *başarısız* bulduğunu doğrular.
- **Unreal yok** (kullanıcı kararı: disk yetmez, kurulum 50+ GB). Unity koşumu (geçici proje ~2–3 GB, sonra silinir) ve Wish koşumu kota harcar → WP8'de kullanıcı onayıyla.
- **Disk:** WP7 başında `target/debug` 104 GB'a şişmişti (cargo eski derleme varyantlarını silmiyor); temizlendi, `tools/clean-build.ps1` ve CLAUDE.md kuralı eklendi.

### WP8 — kapanış incelemesi

İki bağımsız inceleyici (salt okuma; biri motor kodu, biri örnekler + tarifler + değerlendirme + düzenek): **26 bulgu** (motor 8, diğerleri 18), hepsi ele alındı.

- **Motor (regresyon testleri `wish_physics/tests/world.rs`, `wish_control/tests/m7_tools.rs`):**
  - Hareketli platform taşıması duvar/tavan görmüyordu (karakter duvarın içine taşınıyordu) → taşıma karakter şekliyle taranır, ilk engelde kırpılır (`a_moving_platform_does_not_carry_its_character_through_a_wall`, tavan sıkışması testi).
  - Karakterin durduğu platform adımdan *sonra*, yeni pozdaki platforma karşı aranıyordu (hızlı inen platform ışın erimini aşıyordu) → adımdan **önce**, sorgu ağacı henüz eski pozları bilirken bulunur (`a_sinking_platform_keeps_its_character`, 15 m/sn).
  - Temas payı ilk adımdan sonra konuyordu (ilk tick olayları farklı) → çarpıştırıcı kurulurken/eşlenirken konur; yan etkisi (karaktere yaslanan gövdeler 2×pay uzakta durur) belgede.
  - Sahnede karakter yokken de platformların türü her tick değişiyordu (uyumaları engelleniyordu) → yalnızca karakter varken.
  - `asset_create`: `dry_run` ve başarısız çağrı klasör açıyordu → `inside_project` artık yalnızca denetler, klasörü yazmadan hemen önce `prepare_output` açar (capture/golden/replay yazımları da bunu kullanır); `sound`/`picture` içindeki bilinmeyen alan, dizge olmayan ad ve dosya türüne uymayan tanım reddedilir (ipucu yakın adı söyler); diskte başka harf büyüklüğüyle var olan yol reddedilir (Windows'ta çalışıp Linux'ta kırılmasın).
  - `entity_get resources`: yalnızca "dünyada yok" durumunda varsayılan döner ve `resources_defaulted` ile söylenir; başka hata hata olarak döner.
  - Kalan (düşük): dönen platform karakterin yönünü (yaw) çevirmez — not edildi.
- **Tarifler:** test artık her tarifi **oynuyor** (koşucu koşar/zıplar, para toplanır ve yazı değişir, kontrol noktası geri getirir, platform zamanında yerinde, düğme başlatır, 2B top seker; 7 tarif); kontrol noktası tarifinde `respawn` varsayılanı ve "başlangıca ayarla" notu, kazanma tarifinde `won`'u oyunun kurduğu notu.
- **Örnekler:** Bricks'te ilk tuş raketi otomatik pilottan devralır (`keys.test.json`), raket sesi yalnızca rakette; Islands/Bricks bitiş yazısı işaretçi bileşenle (`WinText`/`EndText`) bulunur; Quiz'de "Tekrar" düğmesi test edilir.
- **Değerlendirme görevleri:** 26 kamera dünya konumu + görüntüde oyuncu (çocuk kamera da kabul), 28 hız sıfırlaması denetlenir, 30 gürültü zil sayılmaz, 31 tam 64×64, 32 değer tahmin edilemez (denetçi kendisi okur), 34 z kaymasını kilit durdurmalı, 35 yükseklik nesnenin tepesinden, 37 sensör/çarpıştırıcısız hile kabul edilmez, 38 en yüksek nokta `until` ile, 40 kopyalar aynı olmalı, 41 tam 81 tick; ajanın kendi testi `expect`/`until` içermeli (tüm oyun kodu görevleri). `selftest` **41/41**.
- **Karşılaştırma düzeneği:** Unity kamera denetimi gerçek yatay/dikey görüş açısıyla; düşen kutuda tetikleyici/kilitli Y reddedilir; sahne içi malzeme okunur; iki ajan da boş, tarafsız klasörden ve dosya okuma araçları kapalı koşar; başlangıç sahnesi metin olarak kaydedilir ve okuyucunun nesneleri bulduğu doğrulanır; satırlar biter bitmez yazılır, zaman aşımı koşuyu düşürmez.
