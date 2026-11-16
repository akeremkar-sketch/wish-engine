# M5 — Oyun mantığı: Wasm ev sahibi, SDK, sıcak yükleme, girdi, senaryo testleri

Durum: tasarım (16 Kasım 2026). Dal: `m5-logic`. İlgili kararlar: ADR-005 (determinizm), ADR-006 (anlık görüntü, tekrar), ADR-011 (oyun mantığı Wasm'da), ADR-016 (girdi), ADR-017 (test), ADR-022 (güvenlik), ADR-023 (gömülü araç zinciri).

## 1. Hedef ve sınırlar

M5'in sonunda bir ajan (ya da bir insan) proje klasöründe `scripts/src/lib.rs` yazar, motor onu Wasm'a derler, oyun çalışırken **kaldığı yerden** yeni kodla devam eder, oyun klavyeyle oynanır, bir senaryo testi "kazandın" durumunu kanıtlar ve bir tekrar kaydı 10.000 tick boyunca hash sapmasız oynatılır. Kötü niyetli ya da hatalı konuk kodu (sonsuz döngü, tuzak, bellek patlaması) motoru dondurmaz.

**Kapsamda:** `wish_script` (wasmtime ev sahibi, ABI v1), `wish_sdk` + makrolar (konuk), derleme sürücüsü ve tanı eşleme, sıcak yükleme + şema göçü, `wish_input` (eylem haritaları, enjeksiyon, kayıt), tekrar dosyaları, `wish_test` (senaryo testleri), araçlar `script_build` / `test_run` / `replay`, `sim_step`'e `input`, `edit_apply`'a `session` (M4 devri), `wen play` (pencerede oynanış), sınır maliyeti kıyaslaması, gömülü araç zinciri denemesi.

**Kapsam dışı (bilinçli):** fizik (M6; mini oyun kendi basit çarpışmasını SDK ile yapar), ses (M6), oyun kolu (klavye + fare M5'te; oyun kolu M6'da `wish_input`'a eklenir, `gilrs` lisansı orada denetlenir), ikinci konuk dili (M10), Component Model/WIT (ADR-011 izler), oyun kodu için üçüncü taraf crate'ler (§5.3: ilk sürümde yalnızca `wish_sdk`; M10 listesine), dışa aktarma `wen export` (M9).

## 2. Crate'ler ve bağımlılık yönü

| Crate | Taraf | Sorumluluk | Dış bağımlılık |
|---|---|---|---|
| `wish_script` | ev sahibi | Modül yükleme/doğrulama, manifest, tür kaydı, sistemleri `wish_app`'e bağlama, çağrı çerçevesi, komut tamponu, yakıt/bellek/tuzak, iz → dosya:satır | `wasmtime` (yalnızca burada; değişmez 6) |
| `wish_sdk` | konuk | Oyun kodunun API'si: `Query`, `Res`, `ResMut`, `Commands`, `Events`, `Actions`, `Time`, `Rng`, matematik, `log!` | yok (yalnızca `core`/`alloc`/`std`) |
| `wish_sdk_macros` | konuk (ev sahibinde derlenen proc-macro) | `#[component]`, `#[resource]`, `#[event]`, `#[system]`, `wish_sdk::export!` | `syn`/`quote`/`proc-macro2` (zaten var) |
| `wish_input` | ev sahibi | `input.json`, `ActionState` kaynağı, aygıt olayı → eylem, enjeksiyon, kayıt akışı | — |
| `wish_test` | ev sahibi | Senaryo testleri (`*.test.json`), tekrar doğrulama, sapma arama | — |

Yön: `wish_script`, `wish_input` → `wish_app`, `wish_ecs`, `wish_schema`, `wish_core`. `wish_test` → `wish_script`, `wish_input`, `wish_scene`, `wish_render` (görüntü karşılaştırma için). `wish_control` hepsini kullanır; `wish_window` yalnızca `wish_input`'un olay türlerini tanır. Alt sistemler birbirini tanımaz.

**`wish_app` değişikliği:** `SystemFn` bugün düz `fn` işaretçisi; Wasm sistemi bir modül tutamacı taşır. `System::dynamic(name, Box<dyn FnMut(&mut World, &mut SystemContext) -> Result<()> + Send>)` eklenir; programın parmak izine sistemin **kod parmak izi** (modül içeriğinin `StableHasher` hash'i) girer, böylece "aynı derleme" (D1) Wasm kodunu da kapsar.

## 3. ABI v1 (çekirdek Wasm modülü)

Sözleşme küçük, sürümlü ve tek yönde akan toplu veriye dayanır. Konuk yalnızca `wish` modülünden içe aktarır; **başka her içe aktarma (WASI dahil) modülü reddeder** (`script_forbidden_import`) — dosya, ağ, saat erişimi yapısal olarak yoktur (ADR-011, ADR-022).

**Konuğun dışa aktardıkları:**

| Ad | İmza | Anlam |
|---|---|---|
| `memory` | bellek | Konuğun doğrusal belleği |
| `wish_abi_version` | `() -> i32` | `1`; farklıysa `script_abi_mismatch` + "SDK'yı bu motorla yeniden derle" |
| `wish_manifest` | `() -> i64` | (işaretçi << 32 \| uzunluk): manifest JSON'u |
| `wish_alloc` | `(len: i32) -> i32` | Ev sahibinin çerçeve yazacağı alan |
| `wish_run` | `(system: i32, frame: i32) -> i32` | Bir sistemi çalıştır; dönüş: komut tamponunun (işaretçi) ya da 0 |

**Manifest:** modül adı, SDK sürümü; türler (`wish_schema`'nın şema JSON'uyla **aynı** biçim — bileşen/kaynak/olay, alan belgeleri, birim, aralık, varsayılan); sistemler (ad, sıra, `before`/`after`, sorgular: okunan/yazılan bileşenler, `with`/`without`; okunan/yazılan kaynaklar; okunan/gönderilen olaylar). Ev sahibi manifesti şemaya karşı doğrular; konuk türü motor türüyle aynı adı taşıyorsa (`Transform` gibi) **parmak izleri** aynı olmalı (`script_type_mismatch`).

**Bayt düzeni:** bileşenler `Plain`'dir (işaretçi, `usize`, başvuru yok). wasm32 ile x86_64'te `repr(C)` düzeni bu türler için aynıdır (`f32`/`u32` 4, `u64`/`f64` 8 hizalı), dolayısıyla konuk **ECS sütunuyla aynı baytları** görür (ARCHITECTURE "Wasm ABI yerleşimi"). Bunu varsaymıyoruz, **denetliyoruz**: manifestteki her türün boyut/hizalama/alan ofsetleri ev sahibinin `TypeInfo`'suyla karşılaştırılır. Dolgu baytları asla okunmaz (ECS kuralı): ev sahibi çerçeveye `Column::read_into` ile alan aralıklarını kopyalar, geri yazarken de yalnızca alan aralıklarını alır.

**Çağrı çerçevesi (toplu aktarım, ADR-011):** her `wish_run` için ev sahibi konuk belleğine tek bir çerçeve yazar: başlık (tick, `delta`, sistemin `last_run`'ı, tohum akışı durumu), her sorgu için varlık sayısı + kararlı varlık kimlikleri dizisi + erişilen her bileşen için yoğun dizi (sütun dilimi), kaynak baytları, okunan olaylar. Konuk yazılabilir dizileri yerinde değiştirir. Dönüşte ev sahibi yazılabilir dizileri geri kopyalar; **yalnızca baytları gerçekten değişen** varlıkların değişiklik tick'i ilerler (değişiklik izleme dürüst kalır; `query_since` çalışır).

**Komutlar:** varlık doğur/yok et, bileşen ekle/çıkar, olay gönder; konuk bunları kendi tamponuna yazar (sabit ikili kodlama), ev sahibi çağrıdan sonra **yazıldıkları sırayla** uygular (ADR-005: komut tamponları sabit sırada birleşir). Geçersiz komut (olmayan tür, yönetilen bileşen `Hierarchy`) sistemi tuzak gibi durdurur ve tanı üretir.

**İçe aktarmalar (`wish` modülü):** `log(severity, ptr, len)`, `panic(ptr, len)` (SDK'nın panik kancası: ileti ve `dosya:satır` tanıya girer), `rng_next() -> i64` (sistemin kendi tohumlu akışı — `SystemContext::rng`), `action_index(ptr, len) -> i32` (eylem adı → yuva). Saat, dosya, ağ yok.

## 4. Konuk SDK'sı (`wish_sdk`)

```rust
use wish_sdk::prelude::*;

/// Toplanabilir bir para.
#[component]
pub struct Coin {
    /// Toplanınca verilen puan.
    #[schema(range = 1..=100, default = 1)]
    pub value: u32,
}

#[system(after = "move_player")]
fn collect(mut score: ResMut<Score>, players: Query<&Transform, With<Player>>,
           coins: Query<(Entity, &Transform, &Coin)>, mut commands: Commands) {
    for player in players.iter() {
        for (coin, at, c) in coins.iter() {
            if at.translation.distance(player.translation) < 0.6 {
                score.points += c.value;
                commands.despawn(coin);
            }
        }
    }
}

wish_sdk::export!(collect, move_player);
```

- Makrolar motorun kendi `#[component]` sözdizimini kullanır (aynı öznitelikler, aynı belge yorumu kuralı: belgesiz alan derleme hatası — değişmez 1); üretilen şema manifeste girer. Motor türleri (`Transform`, `Camera`, `Primitive`, `Material`…) SDK'da aynı alanlarla bulunur ve parmak izi testiyle motorla eşit tutulur (her motor türü için bir test: konuk düzeni = ev sahibi düzeni).
- Erişim **imzadan** çıkarılır (`&T` okur, `&mut T` yazar); manifest bunu taşır, ev sahibi yalnızca bildirilen verileri çerçeveye koyar. Bildirilmemiş bir şeye erişmenin yolu yoktur.
- Panik → `wish.panic` → tuzak: tanı `script_panicked` iletiyi ve `scripts/src/lib.rs:42` konumunu taşır.
- Matematik: `Vec2/Vec3/Quat` (SDK içinde, `libm` benzeri belirlenimci işlevler; konuk tarafında `f32::sin` gibi platform işlevleri yerine SDK'nınkiler — D2 hedefi).

## 5. Derleme ve araç zinciri (ADR-023)

### 5.1 Sorun

Oyun kodu kullanıcının makinesinde derlenir. Hedef: **hiçbir geliştirme aracı kurulu olmayan** Windows'ta çalışmak ve kod değişikliğinden çalışan oyuna < 10 sn. Tuzak: proc-macro'lar ve build betikleri **ev sahibi** için derlenir ve Windows'ta MSVC bağlayıcısı (`link.exe`) ister; temiz makinede yoktur. Wasm hedefinin kendi bağlama adımı `rust-lld` ile yapılır (rustc'yle gelir), sorun değildir.

### 5.2 Seçenekler

| | Yol | Ev sahibi bağlayıcısı | Hız | Bedel |
|---|---|---|---|---|
| A | `cargo build --target wasm32-unknown-unknown`, SDK proc-macro'larıyla | **Gerekir** (proc-macro) | cargo çözümleme + artımlı | Temiz Windows'ta çalışmaz — elendi, ancak ölçüm için dursun |
| B | cargo, ama SDK'da proc-macro yok (`macro_rules!` ile bildirim) | Gerekmez | cargo + artımlı | Bildirim sözdizimi Rust'tan uzaklaşır; ajan hataları kötüleşir |
| C | **cargo'suz doğrudan `rustc`**: `wish_sdk` (wasm32 rlib) ve `wish_sdk_macros` (ev sahibi dll) **motorla birlikte önceden derlenmiş** gelir; oyun kodu tek crate olarak `--extern` ile derlenir | Gerekmez (makro dll'i hazır) | En hızlı (çözümleme yok, tek rustc çağrısı) | Önceden derlenmiş parçalar sabit rustc sürümüne bağlı (zaten sabit: `rust-toolchain.toml`); oyun kodu ilk sürümde üçüncü taraf crate kullanamaz |

**Öneri: C**, B yedek. Gerekçe: "sorunsuz" ölçütüne en uygun (ağ yok, çözümleme yok, bağlayıcı yok, en kısa derleme), Rust'ın gerçek öznitelik sözdizimi korunur. Bu bir hipotezdir; **WP1 ölçümle karar verir** ve kararı ADR-025 olarak yazar (M3'teki ADR-013 kanıt işi gibi).

### 5.3 Kanıt yöntemi (WP1 ve WP7)

- **Bağlayıcısız kanıt (otomatik, CI'da da):** derleme `PATH` yalnızca araç zinciri klasörüyle, `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=yok-baglayici.exe` ile ve Visual Studio ortam değişkenleri silinerek koşulur. Ev sahibi için bağlama denenirse derleme başarısız olur → test kırılır.
- **Ölçümler:** soğuk derleme, tek satır değişiklikten sonra derleme, modül yükleme (cranelift), örnekleme; hedef değişiklik → oyun < 10 sn (iddia: C ile < 3 sn).
- **Paket:** `wen toolchain pack` sabit rustup araç zincirinden yalnızca gerekenleri (rustc, `rustc_driver`, LLVM, `rust-lld`, `rust-std-wasm32-unknown-unknown`, önceden derlenmiş SDK) bir klasöre kopyalar; boyut ölçülür ve raporlanır.
- **Temiz makine (elle, WP7):** Windows Sandbox (Windows 10 Pro'da var, ama isteğe bağlı bir Windows özelliği olarak açılması gerekir — kullanıcıya ait adım) ya da başka bir temiz bilgisayar. Paket klasörü kopyalanır, `wen build` + `wen play` denenir.

### 5.4 Tanılar

`rustc --error-format=json` çıktısı ortak tanıya çevrilir: `code` (`rust_E0308` gibi, ya da `rust_error`), ileti, `dosya:satır:sütun` (proje-göreli), `hint` = rustc'nin yardım satırı + bizim ek ipuçlarımız (bilinen SDK hataları için: "sorguda `&mut` kullan", "`export!` listesine ekle"). Yoğunlaştırma: aynı kök hatadan doğan ardıl hatalar ilkinin altında sayılır; ajana ilk 10 hata + toplam sayı. **Derleme başarısızsa eski modül çalışmaya devam eder.**

## 6. Korumalı alan ve determinizm

- **wasmtime ayarı:** `consume_fuel(true)`, `cranelift_nan_canonicalization(true)`, iş parçacığı/rahat SIMD/GC/bileşen modeli kapalı, `StoreLimits` (bellek varsayılan 64 MiB, tablo sınırı), yığın sınırı. Özellikler: `default-features = false`, yalnızca `runtime`, `cranelift`, `std` (+ testlerde `wat`).
- **Yakıt belirlenimcidir:** komut sayar, saate bakmaz → sonsuz döngü **aynı komutta** durur, sonuç her koşuda aynı (tekrar ve determinizm bozulmaz). Varsayılan bütçe sistem çağrısı başına (`wish.json` `scripts.fuel_per_call`); aşılırsa `script_out_of_fuel` tanısı + sistem devre dışı + dünya sürer. Duvar saatiyle (epoch) kesme **kullanılmaz**: belirlenimci değildir; yakıt sınırı tek başına dondurmayı engeller.
- **Tuzak** (`unreachable`, sınır dışı bellek, sıfıra bölme, yığın taşması, bellek sınırı) → sistem devre dışı (bugünkü `system_failed` anlamı), tanı `script_trap` + konum (Wasm ad bölümü + DWARF satır tablolarından `dosya:satır`; oyun kodu satır bilgisiyle derlenir).
- **Durumsuzluk zorlanır:** her `wish_run` çağrısı için `InstancePre`'den **taze örnek** (havuz ayırıcıyla mikro saniyeler). Konuk statik değişkende durum tutamaz; tutsa bile bir sonraki çağrıda yoktur. Böylece anlık görüntü eksiksizdir (ADR-006) ve sıcak yükleme kayıpsızdır. WP1 maliyeti ölçer; çok pahalıysa tick başına tek örneğe inilir (karar ADR-025'te).
- **Rastgelelik:** yalnızca `rng_next` (sistemin tohumlu akışı). Zaman: yalnızca çerçevedeki `tick`/`delta`.

## 7. Sıcak yükleme ve şema göçü

```
scripts/src/*.rs değişti (dosya izleyici ya da script_build)
  → derle → hata? tanılar; eski modül sürer
  → doğrula: ABI sürümü, yalnızca `wish` içe aktarmaları, manifest, tür düzenleri
  → tick sınırında: yeni türleri kaydet; değişen türün verisini alan adına göre taşı
      (eski baytlar → eski şemayla JSON → yeni şemayla bayt; yeni alan = varsayılan, silinen alan = düşer, tür değişen alan = varsayılan + uyarı)
  → sistemleri değiştir (devre dışı kalmış sistemler yeniden etkin), program parmak izi güncellenir
  → oyun aynı tick'ten devam eder
```

- Kaldırılan bir bileşen türünün verisi oturum boyunca dünyada kalır (türler oturum içinde silinmez) ve `script_type_removed` bilgisiyle bildirilir; sahne dosyasında hâlâ kullanılıyorsa `check` uyarır.
- **Sahneler oyun türlerini kullanır:** çalışma alanının tür kaydı = motor türleri + son başarılı derlemenin manifestindeki türler (`wish.json`'daki `types.example` yerini `scripts` alır; örnek projeler için `example` kalır). Derlenmiş modül yoksa sahne yine açılır; bilinmeyen bileşenler `check`'te "önce `script_build`" ipucuyla raporlanır.
- Derleme çıktısı `.wish/build/` (proje içi, `wen new`'in `.gitignore`'unda). Proje dosyası dışında hiçbir yere yazılmaz.

## 8. Girdi (`wish_input`, ADR-016)

- `input.json`: eylemler ve bağları. Tür: `button` (basılı/yeni basıldı/bırakıldı), `axis1`, `axis2` (ör. `move` = WASD + ok tuşları, köşegen normalize). Şemalı, kanonik, `check` doğrular (bilinmeyen tuş adı → öneri).
- Dünyada `ActionState` kaynağı (sabit yuvalı, `Plain`): eylemler `input.json`'daki sıralı adlarına göre yuvalara oturur; konuk `actions.axis2("move")` der, ad → yuva eşlemesi çerçeve başlığında gelir.
- Kaynaklar: pencere (klavye + fare; `wish_window` olayları → eylem), **enjeksiyon** (`sim_step` `input` parametresi: `{ "move": [0, 1], "jump": true }` adımın tüm tick'lerinde basılı tutulur; `input_script`: tick → değer listesi), tekrar dosyası. Girdi tick başında `ActionState`'e yazılır (ARCHITECTURE kare döngüsü 1. adım) — oyun kodu ham aygıtı hiç görmez.

## 9. Tekrar kayıtları (ADR-006)

- `replays/<ad>.replay.jsonl`: başlık satırı `{format, scene, scene_hash, seed, tick_rate, code_fingerprint}`, ardından yalnızca **değişen** eylem değerleri `{tick, actions}` ve her 60 tick'te bir `{tick, hash}` denetim noktası, sonda son hash.
- Oynatma: denetim noktalarında hash karşılaştırılır; sapma varsa son eşleşen noktadan tick tick koşulup **ilk sapan tick** ve o tick'te değişen varlıklar raporlanır (`replay_diverged`). Kod parmak izi farklıysa `replay_code_changed` uyarısı (oynatma yine yapılır: "kodu değiştirdim, oyun hâlâ aynı mı?" sorusunun aracı).
- Kayıt: oturum her tick'in eylem durumunu zaten tutar; `replay save` diske yazar. `wen play --record`.

## 10. Senaryo testleri (`wish_test`, ADR-017)

```json
{ "format": 1, "scene": "scenes/main.scene.json", "seed": 1,
  "steps": [
    { "input": { "move": [1, 0] }, "frames": 120 },
    { "expect": "Score.points >= 3", "message": "player collects the three coins" },
    { "until": "GameState.won == true", "max_frames": 600 },
    { "capture": { "camera": "camera", "golden": "tests/golden/win.png", "tolerance": 2 } }
  ] }
```

- `tests/*.test.json`, başsız ve belirlenimci. `expect` M4'ün ifade dilidir (kaynaklar ve varlıklar; `entity` ile tek varlığa daraltılabilir). Başarısızlıkta: adım numarası, tick, ifade, **alt ifadelerin değerleri** (`Score.points = 2`), yakınlardaki ilgili varlıklar; görüntüde fark resmi `.wish/test-output/`'a.
- Altın görüntüler yazılım bağdaştırıcısıyla ve algısal toleransla (M3 kuralları); `update_golden` yalnızca açıkça istenince.

## 11. Kontrol düzlemi (değişmez 4, 8, 9)

| Araç | Ne yapar | Komut satırı |
|---|---|---|
| `script_build` | Derle; yoğunlaştırılmış tanılar; başarılıysa açık oturumlara sıcak yükle (`reload`, varsayılan açık); süre ve modül parmak izi | `wen build` |
| `test_run` | Senaryo testleri; `paths`, `update_golden` | `wen test` |
| `replay` | `save` (oturumdan), `run` (doğrula, ilk sapma), `info` | `wen replay` |
| `sim_step` + `input` | Adım boyunca eylem enjekte et | `wen call sim_step --input …` |
| `edit_apply` + `session` | Canlı oturuma işlem uygula (M4 devri); kalıcı değil, yanıtı bunu söyler | — |

- `project_info` betik durumunu da verir (derlendi mi, parmak izi, sistemler, son derleme hataları); `wish://docs/sdk` kaynağı ve kılavuzun "oyun kodu" bölümü; `wen new` `scripts/` iskeleti (çalışan küçük bir sistem ve bir test) ve `input.json` ile gelir.
- **`wen play [--scene] [--record ad]`**: pencerede gerçek zamanlı oynatma (klavye → eylemler), `scripts/` izlenir ve kaydedilince derlenip **oyun durmadan** yüklenir; hata başlıkta ve stderr'de. İnsan için oyunu oynamanın yolu; ajan için aynı şey `sim_*` + `capture_frame`.
- Her yeni tanı kodu `docs::DIAGNOSTICS`'e; her yeni araç eşlik testine örnekle (M4 kuralları).

## 12. Test planı (çıkış ölçütleriyle)

| Çıkış ölçütü | Kanıt |
|---|---|
| Claude, MCP + dosya düzenlemeyle oynanabilir mini oyun yazar (hareket, topla, kazan) ve senaryo testiyle kanıtlar | (a) depoda örnek proje `examples/collect` + senaryo testi CI'da; (b) `evals/` yeni görev(ler): ajan `scripts/` dosyalarını düzenleyebilir (Read/Edit/Write yalnızca proje içinde, Bash yok), denetim gizli bir senaryo testiyle (**koşum kullanıcı onayıyla**) |
| Kod değişikliği → çalışan oyun < 10 sn; durum korunur | Betikli test: oturum 300 tick ilerler → kaynak değişir → `script_build` → aynı tick, aynı varlık değerleri, yeni davranış; süre ölçülüp kaydedilir (debug/release, soğuk/sıcak) |
| Sonsuz döngü ve tuzak motoru dondurmaz/çökertmez, tanı üretir | Düşmanca konuk paketi (`wish_script/tests/hostile.rs`, el yazımı `.wat` + Rust konuklar): sonsuz döngü, derin özyineleme, bellek patlaması, sınır dışı erişim, sıfıra bölme, panik, geçersiz komut, yasak içe aktarma, bozuk manifest, yanlış ABI, bozuk Wasm baytları → hepsi tanı, motor sürer, **aynı girdide aynı tick'te aynı tanı** |
| Kaydedilen tekrar 10.000 tick hash sapmasız | `wish_test` testi: mini oyun 10.000 tick kayıt → iki ayrı süreçte oynatma → her denetim noktası eşit; bilerek bozulmuş kodla sapmanın ilk tick'i doğru bulunur |
| Sınır maliyeti kıyaslaması kaydedildi (ADR-011 tetikleyicisi) | `wen bench --script`: 1/1.000/10.000 varlıkta yerel sistem vs Wasm sistemi, çağrı başına sabit maliyet ve varlık başına maliyet; sonuç tasarım notunda; kare bütçesinin (%20 = 3,3 ms) aşılıp aşılmadığı |
| Gömülü araç zinciri, temiz Windows'ta oyun kodunu derleyip çalıştırır | Otomatik: bağlayıcısız + yalnızca paket klasörüyle derleme testi (CI); elle: temiz makinede deneme (WP7, kullanıcı adımı) |

## 13. İş paketleri ve önerilen model

| # | İş paketi | Model · efor |
|---|---|---|
| WP1 | **Kanıt işi:** asgari wasmtime ev sahibi (yakıt, bellek, tuzak, taze örnek maliyeti), derleme yolları A/B/C ölçümü + bağlayıcısız kanıt, ilk sınır maliyeti ölçümü → **ADR-025** | Opus 5.5 · high |
| WP2 | ABI v1: `wish_sdk` + `wish_sdk_macros`, `wish_script` (manifest, tür/düzen doğrulama, çerçeve, komutlar, olaylar, kaynaklar, rng, log, panik, iz → satır), `wish_app` dinamik sistemler | Opus 5.5 · high |
| WP3 | Derleme sürücüsü + tanı eşleme, `script_build` / `wen build`, çalışma alanı türleri manifestten, `wen new` iskeleti, sıcak yükleme + şema göçü | Opus 5.5 · high |
| WP4 | `wish_input`: `input.json`, `ActionState`, SDK `Actions`, `sim_step input`, pencere klavye/fare → eylem, `wen play` (izle → derle → yükle) | Sonnet 5.5 · medium |
| WP5 | Tekrar dosyaları: kayıt, oynatma, sapma arama, `replay` / `wen replay`, 10.000 tick kanıtı | Sonnet 5.5 · high |
| WP6 | `wish_test`: senaryo testleri, `test_run` / `wen test`, görüntü karşılaştırma; `edit_apply` oturumda | Sonnet 5.5 · medium |
| WP7 | Sınır maliyeti kıyaslaması (ADR-011 tetikleyicisi); gömülü araç zinciri paketi (`wen toolchain pack`) + bağlayıcısız CI testi + temiz makine denemesi (ADR-023 tetikleyicisi) | Opus 5.5 · high |
| WP8 | Mini oyun örneği `examples/collect`, belgeler (kılavuz, `wish://docs/sdk`), değerlendirme görevleri + **onaylı** koşum, bulgulara göre düzeltme | Opus 5.5 · high (düzenek); değerlendirilen Sonnet 5.5 · medium |
| WP9 | Düşmanca konuk ve bulanıklaştırma testleri, bağımsız kapanış incelemesi, M5 sonucu, `main`'e birleştirme | Opus 5.5 · high |

## 14. Riskler

- **Gömülü araç zinciri** (ADR-023'ün "en büyük ürün riski"): boyut (tahmin 200–300 MB açılmış), antivirüs yanlış alarmı, ilk derleme süresi. Önlem: WP1'de erken ölçüm; tutmazsa ADR-023 tetikleyicisi (önceden derlenmiş SDK + daha hafif yol ya da ikinci dil) devreye girer.
- **Sınır maliyeti:** her çağrıda taze örnek + çerçeve kopyası. Önlem: WP1/WP7 ölçümü; ADR-011 tetikleyicisi (paylaşımlı bellek görünümleri).
- **wasm32 ↔ x86_64 düzen farkı:** varsayım değil denetim (§3); her motor türü için düzen eşitliği testi.
- **Kayan nokta D2:** konukta platform matematiği yok (SDK işlevleri), NaN kanonikleştirme; D1 garantisi zaten aynı derlemeyle sınırlı.
- **wasmtime sürüm hızı** (ayda bir ana sürüm): `wish_script` dışına sızmaz; sürüm sabitlenir, güncelleme ayrı commit.
- **rustc sürüm bağımlılığı (seçenek C):** önceden derlenmiş SDK, `rust-toolchain.toml` değişince yeniden üretilir; sürüm uyuşmazsa `script_toolchain_mismatch` + ipucu.

## 15. Kullanıcıya ait kararlar / onaylar

1. **İndirme (şimdi, WP1 öncesi):** sabit araç zincirine `wasm32-unknown-unknown` hedefi (`rustup target add wasm32-unknown-unknown`, ~30 MB, static.rust-lang.org). Cargo bağımlılıkları (`wasmtime` 50.0.1, Apache-2.0 WITH LLVM-exception — `deny.toml`'da izinli) derleme sırasında crates.io'dan gelir.
2. **CI:** iş akışına aynı hedefin eklenmesi (GitHub Actions süresi biraz artar).
3. **WP7:** temiz makine denemesi — Windows Sandbox özelliğini açmak (Windows ayarı, kullanıcı yapar) ya da başka temiz bir bilgisayar.
4. **WP8:** değerlendirme koşumu abonelik kotası harcar — koşmadan önce onay.

## 16. Açık sorular

1. ~~Örnek başına çağrı mı, tick başına örnek mi?~~ → WP1: tick başına (ADR-025).
2. Oyun kodunda üçüncü taraf crate'ler: seçenek C'de yok. İhtiyaç değerlendirmelerde görülürse cargo yolu (B) yan yol olarak eklenir; M10 listesine not.
3. `ActionState` yuva sayısı (öneri 64) ve eylem adı uzunluğu.

## 17. Uygulama notları

### WP1 — kanıt işi: ev sahibi, örnekleme maliyeti, derleme yolu (ADR-025)

- **Karar: ADR-025** ([DECISIONS.md](../DECISIONS.md)): derleme yolu **C** (cargo'suz tek `rustc` çağrısı, önceden derlenmiş SDK, ev sahibi bağlayıcısı yok) ve **modül başına tick başına taze örnek** (başarısız çağrıdan sonra yenisi). §5.2'deki hipotez ölçümle doğrulandı; §6'daki "çağrı başına mı, tick başına mı" sorusu (açık soru 1) kapandı.
- **`wish_script` (ilk hali):** `ScriptEngine` (yakıt açık, NaN kanonikleştirme, rahat SIMD kapalı / belirlenimci, memory64 ve çoklu bellek kapalı, 512 KiB Wasm yığını, havuz ayırıcı 64 yuva; wasmtime `default-features = false`, iş parçacığı ve bileşen modeli derlenmedi), `ScriptModule::load` (derle, yalnızca `wish.log/panic/rng_next` içe aktarmaları, gerekli dışa aktarmalar, ABI sürümü), `ScriptModule::run(system, frame, rng)` (taze örnek, çerçeveyi kopyala → `wish_run` → geri kopyala; sistemin rastgele akışı ilerler). Şimdilik çağrı başına örnek; tick başına örnek WP2'de `wish_app` bağlantısıyla gelir.
- **Hata eşleme:** `script_out_of_fuel`, `script_out_of_memory` (kendi `ResourceLimiter`'ımız isteği kaydeder, konuk -1 görür ve Rust'ın ayırıcısı tuzak atar), `script_stack_overflow`, `script_trap`, `script_panicked`, `script_call_failed`, yükleme hataları (`script_invalid_module`, `script_forbidden_import`, `script_missing_export`, `script_abi_mismatch`). **Konum:** panikte SDK kancasının gönderdiği `dosya:satır:sütun` (ABI: `panic(ileti, dosya, satır, sütun)`); tuzakta Wasm izinin DWARF satır tablolarından **ilk oyun kodu çerçevesi** (std/registry çerçeveleri atlanır) ve işlev adı (`trap_here`). Günlük çağrı başına 100 iletiyle, ileti 2000 baytla sınırlı; saçma işaretçiler boş metin olur, ev sahibini düşüremez.
- **Testler:** `tests/hostile.rs` (el yazımı `.wat`: sonsuz döngü, `unreachable`, sonsuz özyineleme, 4 GiB bellek isteği, sıfıra bölme, panik, 1000 günlük, sınır dışı yazma, saçma işaretçi; WASI/saat içe aktarması, dışa aktarması olmayan modül, eski ABI, yanlış imza, bozuk baytlar) — her hata ipuçlu, **aynı girdide aynı tanı**, ardından modül yine çalışıyor; global sayaç her çağrıda 1 (taze örnek kanıtı); yakıt kullanımı çağrılar arasında aynı. `tests/rust_guest.rs`: gerçek Rust konuğu (test sırasında wasm32 için derlenir): çerçeve gidiş-dönüşü, `index out of bounds` ve `unwrap` panikleri doğru satırla, `unreachable` tuzağı doğru satır ve işlev adıyla.
- **Ölçümler** (release, yeniden üretmek için `--ignored --nocapture`): `rust_guest.rs::measure_call_cost`, `module.rs::profile` (adım dökümü), `build_paths.rs` (A/C yolları, boş ortamda kopyalanmış araç zinciriyle derleme, boyut). Sayılar ADR-025'te. Konuk yığınını 1 MiB'tan 64 KiB'a indirmek ve başlangıç belleğini küçültmek ölçülebilir bir fark yaratmadı (gürültü içinde); örnekleme maliyeti bellek boyutundan değil wasmtime'ın örnek kurulum/bırakma işinden.
- **Ortam:** `wasm32-unknown-unknown` hedefi kullanıcı onayıyla kuruldu (16 Kasım 2026) ve `rust-toolchain.toml`'a eklendi (CI ve yeni makineler kendiliğinden kurar).
- **Not:** ölçüm konukları `crates/wish_script/guests/` altında, çalışma alanının dışında (her biri kendi `[workspace]`'i ile); `spike` el yazımı ABI'dir, WP2'de SDK'nın ürettiğiyle değiştirilecek.

### WP2 — ABI v1: SDK, makrolar, manifest, çerçeve, komutlar, dinamik sistemler

- **Önemli karar — tek şema kaynağı (değişmez 1, ADR-003):** konuk, motorun **kendi** şema makrolarını (`#[component]`, `#[resource]`, `#[event]`, `#[schema]`) ve `wish_schema`'yı kullanır (ADR-025'in önceden derlenmiş SDK yolu sayesinde bağımlılık sorun değil). Motor türleri yeni **`wish_types`** crate'ine taşındı (`Transform`, `GlobalTransform`, `Camera`, `DirectionalLight`, `Shape`, `Primitive`, `Material`, `Model`, `Sky`, `Ambient`, `Exposure`, `Time`; ECS'ye bağımlı değil, wasm32'de derlenir); `wish_transform`, `wish_render`, `wish_app` onları yeniden dışa aktarır (dış yollar ve hash'ler değişmedi). Böylece konuğun gördüğü bildirim motorunkiyle **aynı kaynak koddur**; ayrı bir "SDK kopyası" ve sapma riski yok. §4'teki "SDK'da aynı alanlarla" planının yerine geçti.
- **Tel biçimi:** `wish_schema::wire::{to_wire, from_wire}` — `TypeSchema`'nın birebir gidiş-dönüş yapan JSON'u (her motor türü ve her alan türü için testli, parmak izleri aynı); `from_wire` derinliği sınırlar ve sonucu `TypeSchema::validate` ile denetler (düşmanca açıklama reddedilir: `invalid_type_description`).
- **Manifest (`wish_manifest` → JSON):** `abi`, `sdk`, `types` (rol + tel biçiminde şema; sistemlerin kullandıkları kendiliğinden, `export!`'te listelenenler ek olarak — sahnelerin kullanacağı ama hiçbir sistemin sorgulamadığı türler için), `systems` (ad, `before`/`after`, parametreler: `query {columns [{type, write}], with, without}`, `res {type, write}`, `events {type}`, `send {type}`, `commands`). Ev sahibi okur (`Manifest::parse`, sınırlar: 256 sistem, 1024 tür, 32 parametre, 4 MiB) ve `GameModule::install` türleri kaydeder: aynı adlı tür dünyada varsa **düzeni aynı olmalı** (`register_*_schema` zaten bunu yapıyordu) — motor türleri böyle denetlenir (`script_type_mismatch`).
- **Çerçeve (`wish_sdk::frame`, iki tarafın ortak kodu):** başlık (`WSHF`, sürüm, tick, bölüm tablosu) + parametre başına 8 hizalı bölüm: sorgu = sayı, sütun sayısı, varlık bitleri, sonra her bileşenin yoğun sütunu; kaynak = değerin baytları (dünyada yoksa ev sahibi **varsayılanı ekler**); olaylar = sayı + değerler (**sistemin son çalışmasından beri** gönderilenler: `World::event_bytes_since(last_run)`, `last_run` anlık görüntüde olduğu için geri yüklemede doğru). Komut ve olay yazıcı bölümleri boş.
- **Konuk tarafı:** `Query<D, F>` (`D`: `Entity`, `&T`, `&mut T`, 8'e kadar demet; `F`: `With`, `Without`, demetler) sütunları `Plain::decode` ile çözer, `iter`/`iter_mut`/`get`/`get_mut`/`single`/`entities`; **`&mut` sütunlar ve `ResMut` bırakılırken (`Drop`) çerçeveye geri yazılır.** `#[system]` (yeni `wish_sdk_macros`, syn) parametre türlerindeki örtük referans ömürlerini `'static` işaretçiye çevirir ve fonksiyonla aynı adlı bir işaretçi tür (tür ad alanında) üretir; `export!` (macro_rules) dört dışa aktarmayı yazar. Konukta panik = hata bildirme yolu (`abi::fail`); panik kancası iletiyi ve satırı ev sahibine yollar.
- **Komutlar (`wish_sdk::command`, ortak kod):** spawn (çağrı içi sıra numarası), despawn, insert, remove, send, set_parent, remove_parent; türler **adla** (konuğun çalışma anında tür tablosu kurmasına gerek yok); çözücü her uzunluğu denetler (bozuk baytlar hata, panik değil). Ev sahibi verilen sırayla uygular; başarısız komut sistemi durdurur (`script_command_failed`; öncekiler uygulanmış kalır — yerel bir sistemin yarıda hata vermesiyle aynı anlam).
- **Geri yazma dürüst:** ev sahibi her yazılabilir değeri dünyadakiyle karşılaştırır, yalnızca **gerçekten değişeni** yazar (değişiklik izleme doğru kalır) ve dışarıdan gelen her veri gibi doğrular (`insert_bytes`: bool/enum baytları, metin tutamaçları; `script_write_failed`). `wish_ecs`'e güvenli kodla eklenenler: `component_bytes_into`, `resource_bytes`, `insert_resource_bytes`, `send_bytes`, `event_bytes_since` (doğrulama `check_outside_bytes`'ta tek yerde).
- **`wish_app`:** `System::dynamic(ad, Arc<dyn Fn>)`, `App::replace_system` (aynı adlı sistemin yerine geçer, **rastgele akışını ve `last_run`'ını korur**, devre dışıysa yeniden açar), `App::remove_system`. **Karar (§2'deki plandan değişiklik):** kod parmak izi programın parmak izine **katılmaz** — anlık görüntüler sıcak yüklemeden sonra da geri yüklenebilir ("geri sar, kodu düzelt, aynı anı yeniden dene"); kod parmak izi ayrıca raporlanır (tekrar başlığında `code_fingerprint`, WP5).
- **Tick başına örnek (ADR-025):** `ScriptRuntime` (uygulama başına bir tane; iki oturum asla örnek paylaşmaz) tick'in örneğini tutar; tick değişince, bir sistem aynı tick'te ikinci kez çalışınca (geri yükleme demektir) ya da bir çağrı başarısız olunca yenisi açılır. Konuk günlükleri `take_logs()` ile (tick, sistem, önem, ileti).
- **`unsafe`:** `wish_sdk`'de yalnızca konuk sınırında (`frame.rs`: çerçeve işaretçisinden dilim; `abi.rs`: içe aktarmalar, ayırma). Güvenlik gerekçesi: çerçeveler örnek yaşadıkça hiç serbest bırakılmaz, bölümler örtüşmez, her bölümün tek yazarı vardır. Miri ile (`wish_sdk/tests/frame.rs`) denetlenir. CLAUDE.md'deki izin listesine eklendi.
- **Testler:** `wish_script/tests/game.rs` — SDK ile yazılmış mini oyun (`guests/collect`: hareket, para toplama, puan, olaylar, komutla doğan bonus, `After` sıralaması) gerçek bir `App`'te: 60 tick sonunda oyuncu x = 6 m, puan 13, dört para; aynı başlangıç → aynı hash; anlık görüntüye dönüp tekrar koşmak ve kodu iki tick arasında yeni bir çalışma zamanıyla değiştirmek **kesintisiz koşuyla aynı hash**; panikleyen sistem `script_panicked` + `src/lib.rs` + **doğru satır**, sonsuz döngü `script_out_of_fuel`, ikisi de kapatılır ve oyun aynen sürer. `wish_types/tests/wire.rs` (tel biçimi), `wish_sdk/tests/frame.rs` (sorgu/kaynak/olay çözme ve geri yazma, komut gidiş-dönüşü, bozuk çerçeve/komut reddi), `wish_app` dinamik sistem testi. Toplam 385 test.

### WP3 — derleme sürücüsü, `script_build`, türler, sıcak yükleme ve şema göçü

- **SDK paketi (`wish_script::build::pack_sdk`, `wen sdk`):** `cargo build -p wish_sdk --target wasm32` çıktısından oyun derlemesinin gerektirdiği her şeyi bir klasöre toplar: `wasm/` (wasm32 rlib/rmeta'lar), `host/` (proc-macro kütüphaneleri: `wish_sdk_macros`, `wish_schema_macros`, `serde_derive`), `sdk.json` (paketi derleyen rustc sürümü, oyun kodunun adlandırdığı crate'lerin dosyaları: `wish_sdk`, `wish_schema`). Paketlenmiş motorda `sdk/` program yanında gelir; geliştirmede `wen sdk` (varsayılan: `wen`'in yanına) ya da `WISH_SDK`.
- **Derleme (`build_game`):** ADR-025 yolu C — `rustc --crate-type cdylib --target wasm32-unknown-unknown -C opt-level=1 -C panic=abort -C debuginfo=line-tables-only -C incremental=.wish/build/incremental --extern wish_sdk=… --extern wish_schema=… -L dependency=sdk/wasm -L dependency=sdk/host --error-format=json scripts/src/lib.rs`, **boş ortamla** (`env_clear`; yalnızca derleyicinin klasörü ve Windows'un zorunluları) — aynı kaynak her makinede aynı modülü verir. Çıktı önce `.wish/build/game.next.wasm`'a; başarılıysa `game.wasm`'ın yerine geçer, **başarısız derleme önceki modülü bozmaz**. Derleyici bulunamazsa / sürüm tutmazsa: `script_toolchain_missing` / `script_toolchain_mismatch` (SDK ile derleyici aynı paketten gelmeli).
- **Tanılar:** rustc'nin JSON iletileri → `script_compile_error` / `script_compile_warning`; ileti `[E0308] …`, konum proje-göreli (`scripts/src/lib.rs`, satır, sütun — derleyici proje kökünde çalıştırıldığı için yollar zaten öyle), ipucu = rustc'nin `help`/`note` alt iletileri (önerilen düzeltme dahil) ya da `rustc --explain <kod>`. En çok 20 ileti + toplam sayılar.
- **Ölçüm (geliştirme makinesi, debug test):** ilk derleme 2,4 sn, **tek satırlık düzenlemeden sonra 0,7 sn** (artımlı). Çıkış ölçütü "< 10 sn" rahatça tutuyor (pencerede yükleme ölçümü WP4'te `wen play` ile).
- **Kontrol düzlemi:** `Workspace` derlenmiş modülü tutar (`script.rs`): proje açılırken `.wish/build/game.wasm` varsa yüklenir (yeniden açılan proje oyunun türlerini derlemeden bilir); türler **tek yerden** kaydedilir (`types::register_all`: motor + örnek + modül) — belge görünümleri, tür kaydı ve oturumlar aynı işlevi kullanır; belge görünümünün önbelleği modül değişince yenilenir. Oturumlar modülün sistemlerini bir `ScriptRuntime` ile çalıştırır, `transform_propagation` hepsinin ardından; oyun kodunun günlükleri `logs_get`'te `kind: script`.
- **`script_build` aracı (21. araç) / `wen build`:** `reload` (varsayılan açık), `dry_run` (yalnızca derlenir ve yüklenebildiği denetlenir, hiçbir şey değişmez). Yanıt: `passed`, `errors`, `warnings`, `seconds`, `systems`, `fingerprint`, `diagnostics` (derleyici iletileri + oturum notları: `script_reloaded`, `script_type_changed`, `script_reload_failed`). Kaynak yoksa `script_sources_missing` (eşlik testi ortamdan bağımsız kalsın diye derleyici aranmadan önce). `project_info` artık `scripts` alanı taşır (kaynak, modül: dosya, parmak izi, sistemler, oyunun kendi türleri; son derleme; yükleme sorunu).
- **Sıcak yükleme:** yeni modül her açık oturuma tick sınırında girer: türler kurulur, eski modülün artık olmayan sistemleri çıkarılır, kalanlar `replace_system` ile değişir (**rastgele akış ve `last_run` korunur**), yeni olanlar eklenir, `transform_propagation` yeniden sıralanır. Dünya hiç dokunulmadan kalır → oyun kaldığı yerden yeni davranışla sürer (testte: dönen topaç yeni kodla geri döner, açı kesintisiz).
- **Şema göçü (`wish_ecs::World::retype_component` / `retype_resource` / `retype_event`):** oyunun bir türü alan değiştirince her değer eski şemayla JSON'a, oradan **alan adına göre** yeni düzene taşınır (aynı ad + hâlâ uyan değer: korunur; yeni alan: varsayılan; gerisi `Retyped::dropped`'ta raporlanır); ekleme/değişme tick'leri korunur; olayların tampondaki eski düzendeki örnekleri atılır. Motor türleri (Rust türüne bağlı) ve yönetilen türler reddedilir (`retype_refused`). `GameModule::install` önce **her şeyi denetler** (bir motor türü uyuşmazsa hiçbir şey değişmez), sonra taşır ve `script_type_changed` notu üretir. Belirlenimci: aynı göç iki kez aynı hash'i verir (testli).
- **`wen new`** artık `scripts/src/lib.rs` ile gelir: tek bileşen (`Spin`, rad/s, aralık, varsayılan) ve tek sistemli, okunup üstüne kurulabilecek kadar küçük gerçek bir oyun kodu; proje `CLAUDE.md`'sine "Game code" bölümü, `.gitignore`'a `.wish/`; MCP kılavuzuna (`wish://docs/guide`) "Game code" bölümü.
- **Testler:** `wish_script/tests/build_game.rs` (paket + derleme + çalıştırma, düzenleme süresi, hata satırı ve kodu, uyarı, eski modülün korunması, kaynak yok, sürüm uyuşmazlığı), `wish_control/tests/script_tools.rs` (`wen new` → `script_build` → sahnede `Spin` → oturum → yeni kodla durum korunarak devam + günlük → bozuk derlemede eski kod → `dry_run` → yeniden açılan proje; alan eklenen bileşenin değerlerinin çalışan oyunda korunması), `wish_ecs/tests/retype.rs` (taşıma, tick'ler, rapor, olaylar, kaynaklar, belirlenimcilik, motor türü reddi). Eşlik testinde `script_build` örneği ortamdan bağımsız bir parametre hatası (SDK/derleyici makineden makineye değişir).
- **Gerçek akış (release `wen`):** `wen sdk` → `wen new` → `wen build` 1,96 sn; bir satır değişince **0,41 sn**; hata `scripts/src/lib.rs:20:76` + `[E0277]` (rustc'nin uzun uygulama listeleri ipucunda ilk 3 iletiye / 500 karaktere kısaltılır).
- **Disk bulgusu:** `wasmtime` artık her üst crate'in test ikilisine bağlandığından tam hata ayıklama bilgisiyle derleme klasörü 40 GB'ı aştı ve disk doldu (bağlayıcı LNK1180, ardından bozuk PDB'lerle LNK1285). Çözüm: `[profile.dev.package."*"] debug = "line-tables-only"` (bağımlılıklar için); klasör 8,7 GB. Toplam 392 test.
