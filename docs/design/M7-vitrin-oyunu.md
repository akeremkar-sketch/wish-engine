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
