# M6 — Oyun sistemleri: fizik, ses, ışıklar, animasyon, oyun içi arayüz, parçacıklar, 2D

Durum: tasarım (16 Kasım 2026). Dal: `m6-systems`. İlgili kararlar: ADR-005 (determinizm), ADR-006 (anlık görüntü, tekrar), ADR-011 (oyun mantığı Wasm'da), ADR-012 (render), ADR-014 (fizik: Rapier), ADR-015 (ses: Kira), ADR-017 (test), ADR-020 (3D öncelikli, 2D alt küme). Yeni karar adayı: **ADR-026** (ECS dışı simülasyon durumu, §3).

## 1. Hedef ve sınırlar

M6'nın sonunda bir oyun kodu yazarı (insan ya da ajan) zıplayan, çarpışan, yere basan bir karakter yapar; ses çalar ve ajan sesi duymadan "ne çaldı, nerede, ne kadar yüksek" bilgisini okur; nokta ve spot ışıklarla aydınlatılmış, glTF'den gelen iskelet animasyonlu bir modeli oynatır; ekranda puan yazan ve tıklanabilen bir arayüz gösterir; basit parçacıklar ve 2D bir sahne (ortografik kamera + sprite) kurar. **Her alt sistem kontrol düzleminden gözlemlenebilir ve başsız testlidir** (ADR-017 kuralı); fizikli bir sahne anlık görüntü → geri yükleme ve tekrar determinizm testlerini geçer (Windows ↔ Linux aynı hash).

**Çıkış ölçütleri (ROADMAP):**
1. Her alt sistem kontrol düzleminden gözlemlenebilir ve başsız testli.
2. Fizikli sahne anlık görüntü/geri yükleme ve tekrar determinizm testlerini geçer.
3. Değerlendirme takımı 25 göreve çıkar; Sonnet 5.5 · medium ≥ %85. (Koşum kota harcar: **kullanıcı en sonda yapacak**, 16 Kasım 2026 kararı; görevler `selftest` ile hazır tutulur.)

**Kapsam dışı (bilinçli):** araç fiziği ve eklem düzenleyicisi (eklemler veri olarak var, araç modeli yok), yumuşak cisim/kumaş, 3D ses yankısı/oklüzyon, ses miksaj grafiği (yalnızca kanal grupları), arayüz düzenleyicisi (M8a), animasyon zaman çizelgesi düzenleyicisi (1.0 sonrası, VISION), blend tree / durum makinesi (oyun kodu klipleri kendisi seçer; M10 adayı), GPU parçacıkları (CPU, render tarafında), gölgeli nokta ışıkları (spot ışıkta tek gölge haritası M6 sonu adayı; nokta ışık gölgesi 1.0 sonrası), metin girişi (arayüzde yazı kutusu; M8 ile birlikte), oyun kolu (`gilrs` lisansı denetlenir; WP4'te `wish_input`'a eklenir ya da M7'ye kayar).

## 2. Crate'ler ve bağımlılık yönü

| Crate | Sorumluluk | Dış bağımlılık (lisans) |
|---|---|---|
| `wish_physics` | Fizik bileşenleri (`wish_types`'ta bildirilir), ECS ⇄ Rapier eşitleme, adım, olaylar, sorgular (ışın, şekil, örtüşme), karakter denetleyicisi | `rapier3d` (Apache-2.0), `enhanced-determinism` |
| `wish_audio` | Ses bileşenleri ve olayları, **ses olay günlüğü** (deterministik, dünyada), çıkış (yalnızca pencereli oynatmada) | `kira` (MIT/Apache), çözücüler `hound` (WAV), `lewton` (Ogg Vorbis) — `symphonia` MPL-2.0 olduğu için **kullanılmaz** |
| `wish_ui` | Oyun içi arayüz: düğümler, esnek yerleşim, metin, görüntü, düğme; etkileşim (işaretçi → düğme olayları) | `taffy` (MIT), `fontdue` ya da `ab_glyph` (WP7'de lisans/uygunluk denetimi) |
| `wish_anim` | Klipler (glTF), oynatıcı, iskelet poz hesabı (deterministik, sabit adımda) | — (`wish_import` glTF'i zaten okur) |
| `wish_render` (genişler) | Nokta/spot ışıklar + kümelenmiş forward+, deri (skinning), sprite, ortografik kamera, arayüz çizimi, parçacık çizimi, `colliders` hata ayıklama görünümü | — |

Bileşen bildirimleri (`RigidBody`, `Collider`, `AudioSource`, `PointLight`, `UiNode`, `Sprite`, …) **`wish_types`'ta** durur (M5'te açılan crate: ECS'siz, wasm32'de derlenir) → oyun kodu bunları SDK üzerinden aynen görür. Sistemler kendi crate'lerinde. Yön: `wish_physics`, `wish_audio`, `wish_ui`, `wish_anim` → `wish_app`, `wish_ecs`, `wish_types`, `wish_core`. Birbirlerini tanımazlar; konuşma ECS üzerinden (ör. fizik `Transform` yazar, render okur; arayüz düğme olayı gönderir, oyun kodu okur). `wish_control` hepsini oturuma ekler.

## 3. Fizik (WP1–WP3)

### 3.1 ECS dışı simülasyon durumu (ADR-026 adayı)

Rapier'in durumu (gövde/çarpıştırıcı kümeleri, geniş/dar evre, ada yöneticisi, sıcak başlatma dürtüleri) ECS sütunlarına sığmaz (değişken boyutlu ağaçlar) ve her tick yeniden kurmak hem pahalı hem de kalitesizdir (sıcak başlatma kaybolur, yığınlar titrer). Seçenekler:

| Seçenek | Artı | Eksi |
|---|---|---|
| A. Her tick ECS'den sıfırdan kur | Tek doğruluk kaynağı ECS; anlık görüntü bedava | Pahalı; sıcak başlatma yok → yığınlar titrer; temas olayları kurgulanmalı |
| B. Rapier durumunu dünyada bayt dizisi olarak tut | Dünya hash'i her şeyi kapsar | Her tick serileştirme; `Plain` sabit boyut kuralını bozar |
| **C. Uygulama uzantısı (seçilen)** | Rapier yaşar; anlık görüntü `Clone` ile; ECS gözlenebilir durumu taşır | Hash yalnızca ECS'yi kapsar (aşağıdaki kuralla yeterli) |

**Karar:** `wish_app::App`'e **uzantılar** eklenir: `App::insert_extension::<T: Extension>(value)`; `Extension: Clone + Send + 'static` ve `fn fingerprint(&self) -> Hash128` (isteğe bağlı, tanılar için). `AppSnapshot` uzantıların klonlarını taşır, `restore` geri koyar; çatal (fork) da kopyalar. **Kural (ADR-026):** oyun açısından anlamlı her değer (konum, dönüş, doğrusal/açısal hız, uyku durumu) her tick **ECS bileşenlerine geri yazılır**; uzantı yalnızca türetilmiş/önbellek niteliğindeki durumu (geniş evre ağacı, temas önbelleği, sıcak başlatma) tutar. Böylece dünya hash'i her sapmayı yakalar (bir sapma bir tick içinde konuma/hıza yansır) ve tekrar kayıtları değişmeden çalışır. Uzantı durumu **sapma kaynağı olamaz**: anlık görüntüyle birlikte geri yüklenir; geri yüklemeden sonra devam etmek kesintisiz koşuyla bit düzeyinde aynıdır (test).

**Sıcak yükleme ve düzenleme:** ECS doğruluk kaynağıdır; eşitleme her tick değişiklik izlemeyle (`Changed`, eklenen/silinen) ECS'deki tanımı (gövde türü, şekil, kütle, sürtünme…) Rapier'e taşır; kullanıcı bir `Collider`'ı elle değiştirirse Rapier'deki karşılığı yeniden kurulur. `Transform`'u fizikli bir gövdede elle/kodla yazmak "ışınla" anlamına gelir (dinamikte hız korunur; `teleport` ipucu).

### 3.2 Determinizm

- `rapier3d` `enhanced-determinism` özelliğiyle (platformlar arası aynı sonuç; SIMD ve paralellik kapalı). Paralel adım yok (ADR-005).
- Ekleme sırası belirlenimci: varlıklar sabit sırayla (arketip, satır) işlenir; `Entity ↔ RigidBodyHandle` eşlemesi `BTreeMap`.
- Kayıtlı hash testi: fizikli kutu yığını 600 tick → hash kaydı; CI'da Windows ve Linux aynı hash'i bulmalı (M1'deki kayıtlı hash testleri gibi). Tutmazsa: ADR-014 tetikleyicisi, sorun belgelenir.
- Rapier'in kendi `glam`/`nalgebra` sürümü ile bizimki (`glam` 0.34, `scalar-math` + `libm`) arasındaki sınır yalnızca dizilerle geçilir (`[f32; 3]`); matematik türleri sızdırılmaz.

### 3.3 Bileşenler ve kaynaklar (`wish_types::physics`)

- `RigidBody { kind: Dynamic | Fixed | Kinematic, linear_damping, angular_damping, gravity_scale, can_sleep, ccd, lock: Locks }` — `Locks` bayrakları 2D için (Z ötelemesi, X/Y dönmesi kilitli → 2D fizik ayrı bir motor gerektirmez, ADR-020).
- `Collider { shape: Box | Sphere | Capsule | Cylinder | Plane(yarım uzay yerine ince kutu) | Mesh(model'den, yalnızca Fixed), size: Vec3 (şekle göre), offset, friction, restitution, density, sensor: bool, layer: u32, mask: u32 }`. Ölçek: varlığın `GlobalTransform` ölçeği şekle uygulanır (eşit olmayan ölçek kutu/silindirde desteklenir, küre/kapsülde en büyük eksen; tanı uyarısı).
- `Velocity { linear: Vec3, angular: Vec3 }` — her tick geri yazılır; kod/araç yazarsa gövdeye uygulanır.
- `ExternalForce`/`Impulse`: SDK komutu (`commands.apply_impulse(e, v)`) → bir sonraki fizik adımında (komut olayı); kalıcı kuvvet bileşeni `Force { force, torque }`.
- `CharacterController { up, max_slope_deg, step_height, snap_to_ground, offset }` + `CharacterMotion { desired: Vec3 }` (kodun yazdığı istenen hareket) → çıktı `CharacterState { grounded, ground_normal, hit_wall }` (her tick yazılır; oyun kodu okur). Rapier `KinematicCharacterController`.
- `PhysicsSettings` kaynağı: `gravity` (−9.81 Y), `substeps`, çözücü yinelemeleri.
- Olaylar: `CollisionStarted { a, b }`, `CollisionEnded { a, b }`, `TriggerEntered/Exited` (sensör); sabit sırada (varlık kimliği çiftine göre sıralı) gönderilir.

### 3.4 Sorgular: oyun kodu ve ajan

- **Oyun kodu:** ışın/şekil sorguları bir **içe aktarma** ister (çerçeve önceden kurulur, sorgu çağrı ortasında gelir). `wish.physics_query(ptr, len) -> i64` eklenir (ABI v1 içinde geriye uyumlu ek içe aktarma; eski modüller içe aktarmadığı için etkilenmez). Sorgu, **tick başındaki** fizik durumuna karşı çalışır (salt okunur; sistem sırasından bağımsız → belirlenimci). SDK: `Physics` parametresi: `physics.raycast(origin, dir, max) -> Option<Hit { entity, point, normal, distance }>`, `overlap_sphere`, `shape_cast`. Yakıt: her sorgu sabit maliyet + sonuç sayısı kadar düşülür (belirlenimci).
- **Ajan / araçlar:** yeni araç **`physics_query`** (24. araç; gerekçe: sahne/oturum üzerinde ışın, örtüşme ve temas listesini bir başka aracın parametresine sığdırmak anlamı bulandırır): `{ session, ray: {from, dir, max} | overlap: {shape, at} | contacts: {entity} }` → isabetler sahne kimlikleriyle. `entity_get` fizikli varlıkta `physics` özetini (uyuyor mu, temaslar, `CharacterState`) ek alan olarak verir. `capture_frame view: "colliders"` (MCP_API'de zaten planlı): çarpıştırıcı tel kafesleri, sensörler ayrı renkte.
- **Denetçi (M4'ün dünya denetçisi):** "dinamik gövde zeminin altına düştü" (`fell_out_of_world`, ipucu: zemin çarpıştırıcısı ya da öldürme bölgesi — MCP_API örneği), "çarpıştırıcısız dinamik gövde", "ölçeği sıfır çarpıştırıcı".

### 3.5 Maliyet

WP1 ölçer: 1.000 dinamik kutu (uyumayan), tick başına süre (`enhanced-determinism` SIMD'siz). Bütçe: kare bütçesinin %25'i (4 ms) altında; aşarsa notlanır, M7 vitrin oyunu ölçeğinde yeniden bakılır.

## 4. Ses (WP4)

- **Varlıklar:** `*.wav` (hound), `*.ogg` (lewton) varlık dizinine girer; `AudioClip` meta (süre, kanal, örnekleme hızı) içe aktarmada çıkarılır. Çözme hatası tanıdır (`audio_decode`).
- **Bileşenler:** `AudioSource { clip: AssetRef, volume, pitch, looping, spatial, min_distance, max_distance, autoplay, playing }`, `AudioListener` (genelde kamerada). **Komut/olay:** `PlaySound { clip, volume, pitch, at: Option<Entity>, position }` (tek seferlik).
- **Deterministik çekirdek, deterministik olmayan çıkış:** her tick bir yerel sistem dünyadaki sesleri değerlendirir: başlayan/biten/döngü, dinleyiciye göre **kazanç (dB) ve yön (pan)** hesabı (bizim formülümüzle, sabit adımda) → **ses olay günlüğü** dünyada bir kaynak (`AudioLog`, sabit boyutlu halka: son 256 olay) + oturum günlüğüne (`logs_get kinds: ["audio"]`) satır: "tick 412: `sfx/jump.ogg` başladı, kaynak `player`, 0.8 → dinleyicide −6 dB, sol %40". Ajan sesi duymadan doğrular; senaryo testi `expect` ile sorgular (`AudioLog.last_clip`… ya da ayrı ifade işlevi). Gerçek çıkış (Kira) yalnızca `wen play`'de; çıkış aygıtı yoksa sessizce atlanır, hiçbir zaman simülasyonu etkilemez.
- **Ses düzeyi grupları:** `AudioSettings { master, music, sfx }` kaynağı.

## 5. Işıklar (WP5)

- `PointLight { color, intensity (lm), range }`, `SpotLight { color, intensity, range, inner_angle_deg, outer_angle_deg }` (yön: varlığın −Z'si), yönlü ışık M3'ten.
- **Kümelenmiş forward+:** görüş hacmi 16×9×24 kümeye bölünür; hesaplama gölgelendiricisiyle ışık atama (WebGPU çekirdeğinde var; ADR-019). Küme başına en çok 64 ışık, sahnede 1.024 ışık. Yazılım bağdaştırıcısında da çalışır (altın görüntüler).
- Gölge: yönlü ışık M3'ten; **spot ışık gölgesi** WP5 sonu hedefi (tek harita/ışık, en çok 4); nokta ışık gölgesi kapsam dışı.
- Gözlemlenebilirlik: `describe_view` görünen varlıklarda "bu pikseli aydınlatan ışıklar" değil (pahalı), ama ışık varlıklarının etki küresi/konisi ekrandaysa listelenir; `capture_frame view: "lights"` (ışık etki alanları tel kafes) yeni görünüm.

## 6. İskelet animasyonu (WP6)

- `wish_import` glTF deri (skin) ve animasyonları okur: eklemler sahnede **varlık** olur (hiyerarşi, `Transform`), `Skin { joints: [Entity; N]? }` sabit boyut gerektirir → `Skin` bileşeni eklem sayısını ve model içindeki deri dizinini tutar; eklem varlıkları `Joint { skin_root, index }` taşır (sabit boyutlu bileşenler, değişken listeler hiyerarşiden türetilir).
- Klipler varlık olarak değil **varlık dosyası** (glTF içindeki animasyon, `AssetRef` + ad) olarak kalır; `AnimationPlayer { clip: AssetRef, clip_name: Text, time, speed, looping, playing, weight }` (+ geçiş: `fade_from: Text`, `fade_time`).
- **Sabit adımda, deterministik:** oynatıcı sistemi her tick klibi örnekler ve eklem `Transform`'larını yazar (render ara değerlemesi M3 yolunda). Poz dünyada olduğu için anlık görüntü/tekrar kapsar; oyun kodu eklem konumunu okuyabilir (ör. el kemiğine kılıç bağlamak = hiyerarşi).
- Render: deri gölgelendiricisi (eklem matrisleri depolama tamponunda, varlık başına ofset).
- Gözlem: `entity_get` oynatıcının klibi/zamanı; `describe_view` animasyonlu varlıkta klip adı.

## 7. Oyun içi arayüz (WP7)

- Arayüz **varlıklardır** (hiyerarşi = arayüz ağacı): `UiNode { width, height (Val: px | % | auto), padding, margin, flex_direction, justify, align, gap, position: relative|absolute, offset }`, `UiText { text: Text, size, color, align }`, `UiImage { image: AssetRef, tint }`, `UiButton { action_name? }` → durum `UiInteraction { hovered, pressed, clicked_at_tick }` her tick yazılır; olay `ButtonClicked { entity }`.
- **Yerleşim deterministik ve sabit adımda** (`taffy`): kök ekran boyutu sabit bir sanal çözünürlük (`UiSettings { reference_width, reference_height, scale_mode }`) — pencere boyutu simülasyona sızmaz.
- **İşaretçi girdisi eylemle:** `input.json`'da `pointer` (axis2, sanal çözünürlük pikselinde) ve `click` (button) yerleşik eylemleri; ajan `sim_step input: {"pointer": [640, 360], "click": true}` ile düğmeye basar (ADR-016'nın tek temiz noktası korunur).
- Metin: yazı tipi varlıkları (`*.ttf`/`*.otf`), motorla gelen varsayılan yazı tipi (**lisansı kullanıcı onayına sunulacak**: MIT/Apache/BSD kuralı fontlar için de geçerli mi? — §11), glif atlası render'da.
- Gözlem: `describe_view` arayüz öğelerini ekran kutuları ve metinleriyle listeler ("`score_label`: 'Puan: 3' (12, 8, 140×32)"); `capture_frame` arayüzü çizer.

## 8. Parçacıklar (WP8)

- `ParticleEmitter { rate, burst, lifetime, speed, spread_deg, size_start/end, color_start/end, gravity, seed, max }`.
- **Durumsuz ve deterministik:** parçacık konumu = f(yayıcı tohumu, doğuş indeksi, yaş) — kapalı biçimli; simülasyonda hiçbir şey saklanmaz, render her karede hesaplar. Parçacıklar oyunu etkilemez (çarpışma yok); tekrar ve anlık görüntü bedava, render tick zamanından hesaplar.
- Gözlem: `entity_get` "şu an yaşayan parçacık sayısı" (formülden), `describe_view` yayıcıyı ve kapladığı ekran kutusunu listeler.

## 9. 2D alt kümesi (WP8)

- `Camera`'ya `projection: Perspective | Orthographic` ve `ortho_height` (görünen yükseklik, m) eklenir (şema göçü: yeni alan varsayılanla).
- `Sprite { image: AssetRef, rect (atlas içi), color, flip_x, flip_y, anchor, pixels_per_unit, layer: i32 }`; çizim sırası katman + Z; alfa karıştırmalı.
- 2D fizik: `RigidBody.lock` ile (§3.3). Sprite animasyonu: oyun kodu `rect`'i değiştirir (kare listesi M10 adayı).

## 10. Kontrol düzlemi (değişmez 4, 8, 9)

| Yetenek | Araç yüzeyi |
|---|---|
| Fizik sorguları | **`physics_query`** (yeni, 24. araç), `entity_get` fizik özeti, `capture_frame view: colliders` |
| Fizik/ses/arayüz/animasyon verisi düzenleme | Mevcut `edit_apply` (bileşenler şemadan; elle ve ajanla aynı yol, değişmez 9) |
| Ses gözlemi | `logs_get kinds: ["audio"]`, `entity_get` (çalıyor mu, kazanç) |
| Işık gözlemi | `capture_frame view: lights`, `describe_view` |
| Arayüz | `describe_view` (öğeler, metinler, kutular), `sim_step input: pointer/click` |
| Test | Senaryo testleri (yeni işlevler: ifade dilinde `count` M5'ten; ses için `AudioLog` kaynağı) |

Eşlik testi yeni araç için örnek ister (M4 kuralı); tanı kataloğu her yeni kodu ister.

## 11. Kullanıcıya ait kararlar / onaylar

- **Varsayılan yazı tipi lisansı (WP7):** yaygın açık fontlar SIL OFL lisanslı; proje kuralı "yalnızca MIT/Apache/BSD benzeri". Seçenek: Apache-2.0 sürümlü bir font (ör. eski Roboto/Droid Sans) ya da OFL'i fontlar için kabul etmek. **WP7'de kullanıcıya sorulur**; indirme de onay ister.
- **Ses örnekleri ve test varlıkları:** testler için sesler kodla üretilir (sinüs dalgası WAV'ı) → indirme gerekmez.
- **Değerlendirme koşumları:** kullanıcı en sonda yapacak (16 Kasım 2026).

## 12. Test planı (çıkış ölçütleriyle)

| Ölçüt | Kanıt |
|---|---|
| Fizik determinizmi | Kayıtlı hash (kutu yığını, 600 tick) Windows = Linux (CI); anlık görüntü → geri yükle → devam = kesintisiz; çatal iki oturum aynı hash; 10.000 tick tekrar fizikli oyunla |
| Fizik doğruluğu | Düşen kutu zemin üstünde durur (y ≈ yarı boy), sekme katsayısı, sensör olayları sıralı, karakter yokuş/basamak, ışın sorgusu isabetleri, 2D kilitleri |
| Ses | Günlük satırları (tick, kazanç, pan) altın JSON; döngü/bitiş; çıkışsız ortamda hiçbir şey kırılmaz |
| Işıklar | Altın görüntüler (nokta, spot, 200 ışık kümeleme), yazılım bağdaştırıcısıyla |
| Animasyon | Örnekleme birim testleri (doğrusal/küresel ara değer, adım), Khronos örneği (`RiggedSimple`/`Fox`: indirme onayı gerekir → yoksa kodla üretilen iki kemikli glTF), altın görüntü |
| Arayüz | Yerleşim birim testleri (taffy), tıklama senaryosu (`pointer`+`click` → `ButtonClicked`), `describe_view` metin listesi, altın görüntü |
| Parçacık / 2D | Formül birim testleri, altın görüntüler, ortografik `describe_view` |
| Hepsi | Her yeni tanı kodu katalogda; eşlik testi; düşmanca girdiler (bozuk WAV/OGG/TTF, dev çarpıştırıcı, NaN kuvvet) |

## 13. İş paketleri ve önerilen model

| WP | İş | Model · efor |
|---|---|---|
| WP1 | Kanıt işi: `rapier3d` + `enhanced-determinism`, `App` uzantıları (ADR-026), kutu yığını hash'i (CI'da iki platform), maliyet ölçümü | Opus 5.5 · high |
| WP2 | Fizik bileşenleri, eşitleme, olaylar, karakter denetleyicisi, oturumlara ekleme, şema göçü | Opus 5.5 · high |
| WP3 | Sorgular: `physics_query` aracı, SDK `Physics` + `wish.physics_query` içe aktarması, `colliders` görünümü, denetçi kuralları | Opus 5.5 · high |
| WP4 | Ses: varlıklar, bileşenler, günlük, Kira çıkışı (`wen play`) | Sonnet 5.5 · medium |
| WP5 | Nokta/spot ışıklar, kümelenmiş forward+, spot gölgesi, `lights` görünümü | Opus 5.5 · high (render) |
| WP6 | İskelet animasyonu (içe aktarma, oynatıcı, deri) | Opus 5.5 · high |
| WP7 | Oyun içi arayüz (yerleşim, metin, düğme, işaretçi) | Sonnet 5.5 · high |
| WP8 | Parçacıklar, 2D (ortografik, sprite) | Sonnet 5.5 · medium |
| WP9 | Değerlendirme görevleri 13–25 (`selftest`), fizikli örnek oyun, bağımsız kapanış incelemesi, M6 sonucu, `main`'e birleştirme | Opus 5.5 · high |

## 14. Riskler

| Risk | Önlem |
|---|---|
| Rapier platformlar arası farklı sonuç | `enhanced-determinism`; CI'da kayıtlı hash iki platformda; tutmazsa ADR-014 tetikleyicisi (Jolt değil, önce sorunun kaynağı) |
| `enhanced-determinism` ile yavaşlık | WP1 ölçümü; bütçe %25; gerekirse uyuma eşikleri, alt adım ayarı |
| Uzantı durumu ile ECS'nin ayrışması | ADR-026 kuralı + "geri yükle → devam = kesintisiz" testi her fizik özelliğinde |
| Kümelenmiş ışıkların yazılım bağdaştırıcısında yavaşlığı | Altın görüntüler küçük boyutta; hesaplama gölgelendiricisi WARP/lavapipe'ta doğrulanır |
| Yazı tipi lisansı | §11, kullanıcı kararı |
| Kapsam (M6 = L) | Bağımsız WP'ler; parçacık/2D en sona, gerekirse M7'ye kayar ("bu 1.0 kapsamını büyütmez", kayma notlanır) |

## 15. Uygulama notları

(Her iş paketi bitince eklenir.)

### WP1 — kanıt işi: Rapier determinizmi, ADR-026, maliyet

- **`wish_physics`** açıldı, `rapier3d 0.36.1` (Apache-2.0), özellikler: `dim3`, `f32`, `std`, `enhanced-determinism` (SIMD ve iş parçacığı yok).
- **Determinizm:** 6 katlı kutu piramidi + fırlatılan top, 600 tick, her gövdenin konum/dönüş/hız bitleri `StableHasher` ile: `0x8c77d52755b303cb697ba766c515b051`. **Windows debug = Windows release = Linux (CI)** (kayıtlı hash testi; ilk CI koşusunda tuttu). Tutmazsa ilk bakılacak şey Rapier özellikleri (SIMD/paralel).
- **ADR-026** (uzantılar) uygulandı: `World::insert_extension/extension/extension_mut/remove_extension` (`wish_ecs/src/extension.rs`); anlık görüntü ve `clone` taşır, hash'e girmez. Kanıt: Rapier durumu kopyalanıp devam ettirilince düz koşuyla bit düzeyinde aynı (`a_cloned_state_continues_bit_for_bit`; `PhysicsPipeline` kopyalanamaz, ara belleğidir, yeni bir tane yeterli çıktı).
- **Maliyet (release, yerleşik piramit, 120 tick ortalaması):** 57 gövde 0,04 ms, 212 gövde 0,12 ms, **992 gövde 2,0 ms (kare bütçesinin %12'si)** → %25 bütçesinin altında.
- Not: tasarım §3.1'de "App uzantıları" yazıyordu; uzantılar `World`'de tutuldu (anlık görüntüye otomatik girer, sistemler zaten `World`'e erişir).

### WP2 — fizik bileşenleri, eşitleme, olaylar, karakter denetleyicisi

- **Bileşenler** (`wish_types::physics`, SDK önsözünde de): `RigidBody` (`BodyKind` Dynamic/Fixed/Kinematic, sönüm, `gravity_scale`, uyku, CCD, eksen kilitleri), `Collider` (Box/Sphere/Capsule/Cylinder, `size` × varlığın ölçeği, `offset`, sürtünme, sekme, yoğunluk 1000 kg/m³, sensör, katman/maske), `BodyVelocity`, `BodyForce`, olay `BodyImpulse`, kaynak `PhysicsSettings`, olaylar `CollisionStarted/Ended { a, b, sensor }` (a < b), karakter: `CharacterController`, `CharacterMotion`, `CharacterState`.
- **Ad kararı:** `Velocity`/`Force`/`Impulse` oyunların kendi bileşenleri için çok yaygın adlar (örneklerimiz, M5 test oyunu, değerlendirme görevleri kendi `Velocity`'sini tanımlıyor); motor türü olsaydı her oyun kodu `script_type_mismatch` alırdı. Motorunkiler **`BodyVelocity`, `BodyForce`, `BodyImpulse`**; "velocity/force/impulse" diye arayan ajanı kavram ipucu yönlendirir.
- **`wish_physics::step`** (sistem `physics`, oyun ve örnek sistemlerinden sonra, `transform_propagation`'dan önce; `physics` adı motora ayrıldı): (1) ECS → Rapier: yeni/değişen/silinen çarpıştırıcı ve gövdeler (varlık sırasıyla), değişen çarpıştırıcıda uyuyan gövde uyandırılır; fizik dışından yazılan `Transform` ışınlar, `BodyVelocity` hızı koyar (kilitli eksenler maskelenir — Rapier dışarıdan konan hızı kilitlemiyor); (2) `BodyImpulse` olayları (`World::events_since(last_run)`: konum anlık görüntüde); (3) adım (alt adımlar); (4) karakterler; (5) geri yazma (yalnızca değişen değer yazılır); (6) olaylar. Çocuk varlıklarda dünya ⇄ yerel dönüşüm ebeveynin `GlobalTransform`'u ile.
- **Karakter denetleyicisi — iki bulgu:** (a) yerçekimli tek hareket, şekli zemine bastırıyor ve Rapier yatay kısmı reddediyordu → hareket **iki aşamada** (önce yatay: kayma ve basamak; sonra dikey); basamak çıkılan tick'te aşağı hareket uygulanmaz (yuvarlak taban kenardan geri kaymasın), basamak genişliği kapsül yarıçapına göre. (b) Karakterler adımdan **önce** hareket edince sorgu ağacı o tick eklenen zemini bilmiyor, karakter zemine batıyordu → karakterler adımdan **sonra** taşınır. Yukarı giden karakter `grounded` sayılmaz (zıplamanın ilk tick'i).
- **Kanıtlar:** `wish_physics/tests/world.rs` (düşüp duran kutu, geri yükle → devam = kesintisiz, ECS doğruluk kaynağı, olaylar ve sensör, karakter basamak çıkar ve duvarda durur, 2D kilitleri, JSON; **kayıtlı dünya hash'i** `ffb4e843…` debug = release); `wish_control/tests/physics_tools.rs` (araçlarla kurulan yığın oturumda düşer ve durur, sahne dosyası değişmez; **anlık görüntü, çatal ve 600 tick'lik tekrar bit düzeyinde** — çıkış ölçütü 2; oyun kodu `CharacterMotion` yazıp `CharacterState` okuyarak yürür, zıplar, iner).

### WP3 — sorgular: `physics_query`, oyun kodunda ışın, denetçi

- **`wish_physics::query`:** `PhysicsState::raycast` (ilk isabet: varlık, nokta, normal, uzaklık; sensörler hariç; katman maskesi; sorulan varlık hariç tutulabilir), `overlap` (küre/kutu; sensörler dahil), `touching` (temas + sensör örtüşmesi), hepsi varlık sırasıyla. `query_state(world)`: adım atmış dünyanın kendi durumu, yoksa soru için kurulan durum (yalnızca çarpışma algılama, hiçbir şey kımıldamaz). `prepare(world)`: oturum başlarken durum kurulur; ilk tick'teki sorgular da sahneyi görür ve ilk adım ondan devam eder.
- **`physics_query` aracı (24. araç):** oturumda (son adımdaki dünya) ya da sahne dosyasında (çarpıştırıcılar sahnenin koyduğu yerde); `ray` / `overlap` / `touching`'den **tam bir tanesi**, `mask`; yanıtlar sahne kimlikleriyle; hatalar konumlu (`/ray/from`), bilinmeyen varlık "Did you mean". Eşlik testi (süreç içi = `wen do` = MCP) üç örnekle.
- **Oyun kodunda ışın:** yeni içe aktarma `wish.physics_query(istek, uzunluk, çıktı, kapasite) -> yazılan` (ABI v1'e geriye uyumlu ek; eski modüller içe aktarmaz); istek/yanıt kodlaması `wish_sdk::physics`'te, iki taraf aynı kod. SDK: `raycast(from, dir, max)`, `physics::cast(Ray { .., mask, ignore })`, önsözde. **Tick başındaki dünyaya karşı** (sistemlerden önce physics çalışmaz; uzantı durumu tick boyunca sabit) → sistem sırasından bağımsız, belirlenimci (test: aynı koşu aynı hash). `wish_script` fiziği tanımaz: **`HostServices`** arayüzü (`ScriptRuntime::with_services`), kontrol düzlemi uygular; çağrı süresince dünyaya salt okunur işaretçi (`WorldRef`, `SAFETY` notlu, çağrı biter bitmez temizlenir). Her sorgu sabit **20.000 yakıt**; düşmanca konuk testi: çöp işaretçi/uzunluk, sığmayan yanıt, sonsuz sorgu döngüsü → `script_out_of_fuel`, uygulama sürer.
- **Denetçi:** `rigid_body_without_collider`, `character_not_kinematic`; dinamik gövde dünyadan düşünce `entity_out_of_bounds` ipucu zemin çarpıştırıcısını ve sensörle silmeyi söyler.
- **Taşınan:** `capture_frame view: colliders` (kapsül/silindir ağları ve renkli çizgi geçişi gerekiyor) → render paketi WP5'e.
- Kanıtlar: `wish_physics/tests/world.rs::rays_overlaps_and_contacts_answer_questions`, `wish_control/tests/physics_tools.rs` (araç sahnede ve oturumda, hatalar; oyun kodu ışınla zemine uzaklığı ilk tickte ve yerleşmeden sonra ölçer, tekrar koşu aynı hash; denetçi), `hostile_game.rs::physics_questions_cannot_hurt_the_host`, SDK kodlama gidiş-dönüş testi.

### WP4 — ses

- **Bileşenler** (`wish_types::audio`, SDK önsözünde): `AudioSource` (klip, `AudioBus`, ses, perde, döngü, uzamsal, min/maks uzaklık, `playing`), `AudioListener`, `AudioPlayback` (motor yazar: konum, `gain`, `pan`, `started`), `AudioOneShot` (işaret), kaynak `AudioSettings` (master/sfx/music), olaylar `PlaySound`, `SoundStarted`, `SoundEnded`.
- **`wish_audio::step`** (sistem `audio`; adı motora ayrıldı; **dönüşüm yayılımından sonra**, böylece nesnelerin yeni yerini duyar): `PlaySound` → kısa ömürlü varlık (klip bitince silinir); dinleyici = varlık sırasındaki ilk `AudioListener`; her kaynak için başlat/ilerlet/döngü/bitir; kazanç = ses × bus × master × dinleyici × uzaklık (`min/uzaklık`, `maks`'ta 0), pan = yön · dinleyicinin sağı; hepsi doğrusal genlik ve IEEE temel işlemleri (log yok → platformlar arası aynı). Varlığı yok olan sesin sonu da bildirilir. Bilinmeyen klip bir kez `audio_clip_unknown`, kaynak kapanır.
- **Klip uzunlukları** dünyada değil **uzantıda** (ADR-026): kontrol düzlemi oturum başında projenin `.wav`/`.ogg` dosyalarını okur (`Workspace::audio_clips`; uzunluk dosya boyutu+tarihe göre önbellekte), `wish_audio::install` ile koyar. Çözücüler: `hound` (WAV 8/16/24/32 tam sayı, 32 float), `lewton` (Ogg Vorbis); **Kira'nın varsayılan çözücüsü `symphonia` MPL-2.0 olduğu için kapalı**; sınırlar: 128 MiB dosya, 1 saat, ≤8 kanal (2'ye indirilir); bozuk dosya `audio_decode` (panik yok).
- **Ses olay günlüğü:** sistem uzantıya satır yazar, kontrol düzlemi her adımdan sonra boşaltıp oturum günlüğüne `audio` türüyle koyar (`logs_get kinds: ["audio"]`): "`assets/sounds/jump.wav` started on 'door': -12.0 dB at the listener, right 100%". Anlık görüntüye uzantı girdiği için geri yükle → devam aynı günlüğü üretir (test).
- **Çıkış:** `wish_audio::output::Speaker` (özellik `output`, varsayılan kapalı: cpal Linux'ta ALSA ister) — dünyadan `voices()` okur, **kendi küçük karıştırıcısıyla `cpal` üzerinden** çalar (ses, eşit güçlü pan, perde, döngü; kazanç ve pan yumuşatılır; ses iş parçacığı oyunu asla beklemez), dünyaya **hiçbir şey yazmaz**; **Kira bırakıldı**: `symphonia` kapalı olsa da Kira MPL-2.0 lisanslı `triple_buffer`'a bağlı (CI'daki `cargo deny` yakaladı); cihaz yoksa `audio_device` uyarısı, oyun sessiz sürer. `wen play`'e bağlı, **şimdilik yalnızca Windows** (Linux/macOS paketleri M9). Elle deneme: `cargo test -p wish_audio --features output --test speaker -- --ignored --nocapture` (cihaz açıldı ve komutlar hatasız çalıştı; **kulakla doğrulanmadı**).
- Kanıtlar: `wish_audio/tests/audio.rs` (5: başla/bitir/dB/pan, mesafe ve yön, döngü + tek seferlik + yok olan varlık, bilinmeyen klip, geri yükle = kesintisiz), `clip` birim testleri (WAV, bozuk dosyalar), `wish_control/tests/audio_tools.rs` (sahnedeki ses `logs_get`'te; `check` eksik dosyayı yakalar; anlık görüntü ve tekrar bit düzeyinde).

### WP5 — nokta ve spot ışıklar, kümelenmiş aydınlatma, `colliders` ve `lights` görünümleri

- **Bileşenler** (`wish_types::render`, SDK'da): `PointLight { color, intensity_lm (800 = 60 W ampul), range }`, `SpotLight { color, intensity_lm, range, inner_angle_deg, outer_angle_deg }` (yön: varlığın −Z'si). Lümen → kandela = Φ/4π (spotta da: koni daraldıkça parlaklık değişmez); düşüş ters kare × menzilde yumuşak sıfıra pencere `(1 − (d/r)⁴)²`; spot kenarı iç ve dış kosinüs arasında `smoothstep`.
- **Kümelenmiş forward+ (ADR-012 taban çizgisi):** görüş hacmi 64 px karolar × 24 üstel derinlik dilimi; ışıklar **CPU'da, ışık sırasıyla** kümelere atanır (`wish_render::cluster`: aynı sahne → aynı listeler), gölgelendirici küme listesini üç **salt okunur depolama tamponundan** okur (WebGPU çekirdeği, yazılım bağdaştırıcısında çalışır). Atama muhafazakâr: ışığın sınır kutusunun izdüşümü + derinlikte bir dilim pay (GPU'nun logaritması biraz farklı yuvarlasa da ışık kaçmaz; birim test her aydınlatılan noktanın ışığını kendi kümesinde bulduğunu doğrular). En çok 1.024 ışık, küme başına 128; aşılırsa **uyarı** (`too_many_lights`, `light_clusters_full`), sessiz kesilme yok.
- **Bulgu:** ilk sürümde 200 ışıklı sahnede karo hizalı siyah dikdörtgenler çıktı — dilimler kameranın uzak düzlemine (1 km) yayıldığı için yakın dilimler kalın, küme başına 64 sınırı dolup kameraya en yakın ışıklar atılıyordu (2.364 atama). Dilimler artık en uzak ışığın menziline kadar; karo payı kaldırıldı (kutu zaten muhafazakâr); sınır 128 → **0 atma** (birim test).
- **`capture_frame view: colliders`** (WP3'ten): tel kafes + çarpıştırıcılar (sabit yeşil, hareketli camgöbeği, sensör sarı; kutu/küre/kapsül/silindir, fiziğin kullandığı ölçekle). **`view: lights`**: tel kafes + nokta ışığın menzil küresi, spotun iç/dış konisi, ışığın renginde. İkisi de genel bir **gizmo çizgi geçişiyle** (`wish_render::gizmo`, `gizmo_pass.wgsl`: dünya uzayında renkli çizgiler, her şeyin üstünde) — ileride ışın/yol çizimleri için de.
- **Spot gölgesi yapılmadı:** ADR-012 taban çizgisi yönlü ışık gölgesi (var) ve basamaklı gölge haritaları ister; spot gölgesi M7 vitrin oyunu gerektirirse eklenir (M10/M7 listesine).
- Kanıtlar: `wish_render/tests/lights.rs` (karanlık odada kırmızı ampul ve beyaz spot: renk, koni dışı karanlık, mesafeyle sönme; 200 ışık; iki görünüm; altın görüntüler `lights`, `many_lights`, `view_lights`, `view_colliders`), `cluster` birim testleri, `wish_control/tests/physics_tools.rs::the_colliders_and_lights_views_come_through_the_tools`.
