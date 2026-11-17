# M9 — Ürünleştirme → 1.0

**Durum:** başladı (17 Kasım 2026), dal `m9-product` (`m8b-cockpit` üzerine; M8b'nin kullanıcı adımları bekliyor). Önerilen model · efor: export mimarisi Opus 5.5 · high; belgeler, lisans dosyası, ilk çalıştırma Sonnet 5.5 · medium; yayın denetimi Opus 5.5 · high.

## 1. Hedef ve sınır (ROADMAP)

İçerik:
- tek paket kurulum (gömülü araç zinciri),
- `wen export` (bağımsız oyun paketi),
- ilk çalıştırma akışı,
- örnek projeler,
- kullanıcı belgeleri (TR/EN),
- güncelleyici (web),
- isteğe bağlı çökme raporu,
- üçüncü taraf lisans dosyası,
- son kullanıcı sözleşmesi,
- Steam sayfası ve web sitesi.

**Kullanıcıya ait olanlar:**
- Steam hesabı ve ücreti,
- alan adı ve barındırma,
- kod imzalama sertifikası,
- deponun herkese açılması,
- temiz Windows 10/11 denemeleri.

Bu not, bunları beklemeden yapılabilenleri kapsar.

## 2. Kararlar

### 2.1 Dışa aktarma (`project_export`, 31. araç; `wen export`; editörde "Oyunu dışa aktar")

- Bir **kontrol düzlemi aracı**dır (değişmez 4): editör, komut satırı ve ajan aynı yolu kullanır.
- **Önce denetim:** projede hata varsa dışa aktarılmaz (`export_has_errors`). Oyun kodu varsa ama derlenmemişse dışa aktarılmaz (`export_needs_build`; ipucu: önce `script_build`).
- **Çıktı:** projenin içinde `exports/<ad>/`. Araçlar yalnızca proje içine yazar (M4 kuralı). `exports/` varlık sayılmaz (`wish_asset::NON_ASSET_FOLDERS`). Var olanın üzerine yalnızca `overwrite` ile yazılır.
- **Paket:**
  - `<ad>.exe`: oynatıcının yeniden adlandırılmış kopyası.
  - `game/`:
    - `wish.json`, `input.json`;
    - sahneler ve prefab'lar;
    - varlıklar (varlık dizinindeki her dosya; `scripts/`, `tests/`, `replays/`, `exports/` hariç);
    - `.wish/build/game.wasm`.
  - `LICENSES.txt`: motorun MIT ve Apache-2.0 metinleri, üçüncü taraf listesi.
- **Pakette olmayanlar:** editör, MCP sunucusu, derleyici araç zinciri, oyunun Rust kaynağı (ADR-023).

### 2.2 Oynatıcı (`wish_player`)

- Kütüphane + `wish_player` ikilisi. Oyunu kontrol düzleminin oynatma oturumuyla çalıştırır (`sim_start` / `play_step`); `wen play` ile aynı içerik, ama derleme ve sıcak yükleme kapalı.
- Pencere başlığı projenin adı. Ses: Windows'ta hoparlör.
- `--headless --frames N`: penceresiz N tick koşar ve dünya hash'ini JSON olarak yazar. Dışa aktarılan oyunun projeyle bit bit aynı oynadığını testler bununla kanıtlar.
- Oynatıcının yeri: `wen`'in yanındaki `wish_player(.exe)`; `WISH_PLAYER` ya da `Workspace::set_player_path` başkasını gösterir.
- `wen play`, `wish_player::Game`'i derleme ve sıcak yükleme açık olarak kullanır: tek oyun döngüsü.

## 3. İş paketleri

| WP | İş | Model · efor |
|---|---|---|
| WP1 | Oynatıcı + `project_export` + `wen export` + editörde menü; başsız hash eşitliği testi | Opus 5.5 · high |
| WP2 | Üçüncü taraf lisans dosyası (üretici betik + güncellik testi) | Sonnet 5.5 · medium |
| WP3 | İlk çalıştırma akışı (yapay zekâyı bağla ya da atla → şablon → ilk istek ya da elle) | Sonnet 5.5 · high |
| WP4 | Kullanıcı belgeleri TR/EN, katkı kılavuzu | Sonnet 5.5 · medium |
| WP5 | Paketleme (`wen package` + oynatıcı), temiz makine denemesi (kullanıcı), yayın denetimi | Opus 5.5 · high + kullanıcı |

## 4. Uygulama notları

### WP1 + WP2 — dışa aktarma, oynatıcı, lisanslar (17 Kasım 2026; Opus 5.5 · high)

- **`wish_player`** (kütüphane + ikili):
  - `Game` (pencere içeriği; `wen play`'den taşındı), `play`, `play_headless`, `load_models` (`wen capture` da bunu kullanır).
  - İkili (`wish_player.exe`): `game/` klasörünü ya da `--game <klasör>`'ü oynatır.
  - `--headless --frames N` bir JSON yazar: tick sayısı ve dünya hash'i.
  - Hatalar standart hataya ve `player-error.txt`'ye yazılır. Release'te konsol penceresi açılmaz.
  - `wen play` artık `wish_player::play`'i çağırır (`develop: true`: derleme ve sıcak yükleme açık). Tek oyun döngüsü.
- **`project_export`** (31. araç), `wen export`, editörde "Oyunu dışa aktar":
  - Sıra: önce derlenmemiş kod (`export_needs_build`; aksi hâlde oyunun türleri "bilinmeyen bileşen" diye görünür), sonra `check` (`export_has_errors`), sonra oynatıcı (`player_missing`).
  - Paket: `exports/<ad>/` altında `<ad>.exe` ve `game/` (belgeler, varlıklar, ayarlar, `.wish/build/game.wasm`); `LICENSES.txt`. `dry_run` kopyalanacakları listeler.
  - `exports/` ne varlık ne belge sayılır (`wish_asset::EXPORTS_FOLDER`).
- **`THIRD_PARTY_LICENSES.md`** (depo kökünde, `tools/third_party.py` üretir):
  - Kilitteki 451 kütüphane ve dağıttıkları 211 farklı lisans metni, her metin bir kez (~635 KB).
  - `wish_control::licenses::THIRD_PARTY` ile gömülür.
  - `tests/third_party.rs`, `Cargo.lock`'taki her kütüphanenin dosyada olduğunu denetler (bağımlılık değişince betiği çalıştır).
- `wen package` oynatıcıyı da pakete koyar; yoksa uyarır (`package_without_player`).
- **Kanıt** — `wish_player/tests/export.rs`:
  1. Vitrin oyunu kopyalanır.
  2. Derlenmeden dışa aktarma reddedilir.
  3. Kod derlenir ve oyun dışa aktarılır.
  4. Paketteki program ayrı bir süreç olarak başsız çalışır.
  5. 300 ve 900 tick sonra dünya hash'i projenin kendisiyle **bit bit aynı**dır.

  Ayrıca: hatalı proje ve kötü ad reddedilir; dışa aktarma varlık listesine girmez.
- **Kullanıcıya ait:** paketin temiz bir Windows'ta denenmesi (motor kurulu olmayan makinede çalışır, çıkış ölçütü 2).

### WP3 — ilk çalıştırma akışı (17 Kasım 2026; Opus 5.5 · high)

- **`wen edit` proje olmadan açılınca karşılama ekranı** (`wish_editor::welcome::Launcher`). Proje klasörü: `--root`, yoksa bulunduğun klasördeki `wish.json`.
  - winit süreç başına tek olay döngüsüne izin verir. Bu yüzden karşılama ekranı ve editör aynı pencerede: proje açılınca başlatıcı editöre geçer.
- **Ekran:**
  1. Kim yardım ediyor:
     - Claude Code (bulunduysa yolu gösterilir),
     - ya da kimse (elle).

     Başka bir MCP ajanı sonradan "Dış ajanı bağla" ile bağlanır.
  2. Neyle başlansın: boş proje ya da şablonlar. Vitrin oyunu ilk ve varsayılan.
  3. Ad ve klasör. Varsayılan: `Belgeler/Wish Projects/<ad>`. Ad dile göre önerilir ve boş bir ad seçilir.

  Sağda: son projeler ve klasör yolu ile açma. Dil seçimi sağ üstte.
- **Oluştur:**
  - `scaffold::create_project` ya da yeni **`scaffold::create_from_template`**.
  - Şablondan kopyalanmayanlar:
    - motorun ürettikleri: `.wish/`, `exports/`, `target/`;
    - projeye özel dosyalar: `wish.json`, `.mcp.json`, `CLAUDE.md`, `.gitignore`.

    Bu dosyalar yeni proje için yeniden yazılır.
  - Ardından oyun kodu derlenir (`script_build`): şablon sahneleri oyunun kendi türlerini kullanır.
  - Editör açılır. Yardımcı seçildiyse Ajan paneli açık gelir ve bulunan Claude Code kullanılır.
- **Tercihler** (`editor.json`): dil, son 8 proje, yardımcı seçimi. Her yazan yalnızca kendi anahtarını değiştirir (`wish_editor::prefs`). Editör açtığı projeyi son projelerin başına koyar.
- **Şablonların yeri:**
  1. `WISH_TEMPLATES`,
  2. programın yanındaki `templates/`,
  3. geliştirme derlemesinde deponun `examples/`'ı.

  `wen package` örnekleri `templates/`'e koyar (`scaffold::copy_template`). Komut satırında: `wen new <klasör> --template islands`.
- **Claude Code'u bulma:** `wish_agent::claude::find`. Sırayla:
  1. `WISH_CLAUDE`,
  2. PATH'te `claude.exe`/`claude.cmd` (npm),
  3. `~/.local/bin`.

  Yalnızca bakar, başlatmaz. `ClaudeCode::default` da bunu kullanır.
- **Yeni yetenek yok:** proje oluşturma `wen new` ile aynı iskele işlevi, açma `Workspace::open`. Eşlik tablosu değişmedi. Yeni tanı kodu: `invalid_template`.
- **Kanıt:**
  - `wish_editor/tests/welcome.rs`:
    - şablondan, yardımcıyla oluşturma: oyun kodu derlenir, `check` temiz, Ajan paneli açık, tercihler yazıldı;
    - ikinci açılışta son projelerden açma;
    - elle boş proje (dolu klasörün yerine sonraki ad);
    - proje olmayan klasörün açıklanması.
  - `wish_control/tests/templates.rs`: kopyalanan/kopyalanmayan, geçerli proje, `copy_template`.
  - `wish_cli/tests/parity.rs`: `wen new --template` ve bilinmeyen şablon.
  - `languages.rs`: 11 dil tam.

### WP4 — kullanıcı belgeleri ve katkı kılavuzu (17 Kasım 2026; Opus 5.5 · high)

- **Kılavuzlar:** `docs/user/en/guide.md` ve `docs/user/tr/kilavuz.md`. Aynı on bölüm:
  1. kurulum ve karşılama ekranı,
  2. editör (yerleşim, dolaşma, kısayollar, görünümler),
  3. elle düzenleme,
  4. oynatma / tekrar / test,
  5. oyun kodu,
  6. yapay zekâ (kokpit, izin kipleri, etkinlik, dış ajan),
  7. dışa aktarma,
  8. komut satırı,
  9. sorun giderme,
  10. proje klasörü.
- **Düğme adları gerçek arayüzden alındı:** İngilizce kaynaktaki `t("...")` metinleri ve `lang/tr.json`. Arayüz değişince kılavuz da değişmeli.
  - Türkçe arayüzde "entity" = "nesne", "asset" = "varlık"; kılavuz da böyle der.
- **`CONTRIBUTING.md`** (İngilizce, Türkçe not): derleme ve doğrulama, dokuz değişmez, testlerin zorladığı kurallar tablosu, kod uzlaşımları, çalışma yöntemi.
- **`README.md`** güncellendi: neler yapabildiği, başlarken, kaynaktan derleme, belgeler (Türkçe + İngilizce özet).
- **Kullanıcı belgelerine motor içinden de ulaşılır:**
  - `wish://docs/guide` (ajanlar için),
  - `wish://docs/sdk`,
  - `wish://docs/recipes`,
  - `docs_search`.

  Kılavuz bunlara yönlendirir, tekrar etmez.

### Bağımsız inceleme (WP1–WP4; 17 Kasım 2026)

Kod okuyan bağımsız bir ajan 12 bulgu verdi; hepsi ele alındı. Regresyon testleri:
- `wish_control/tests/review_m9.rs`,
- `wish_editor/tests/parity_ops.rs::the_game_is_exported_from_the_menu`,
- `wish_editor/tests/welcome.rs`,
- `wish_player/tests/export.rs`.

| # | Bulgu | Düzeltme |
|---|---|---|
| 1 (yüksek) | `exports/` içindeki kopyalar `check`, `fmt`, `asset_move` ve izleyicide projenin dosyası sayılıyordu: eski kopya yeniden dışa aktarmayı engelliyor, `wen mv` dışa aktarılan sahneleri yeniden yazıyordu | `wish_asset::index::walk_files` kökteki `exports/`'u hiç görmez |
| 2 | Dışa aktarma eskisini önce siliyordu (yarıda kalan hata eskiyi de götürür); editör her zaman üzerine yazıyordu | Yan klasörde (`exports/.<ad>.exporting`) kurulur, tamamlanınca yer değiştirir. Editör önce sorar: *Önceki dışa aktarma değiştirilsin mi?* (Değiştir / Vazgeç) |
| 3 | `exports` bir bağlantıysa proje dışına yazılıp silinebiliyordu | Bağlantı (Windows'ta bağlantı noktası da) reddedilir, kanonik yol proje içinde olmalı: `export_outside_project` |
| 4 | Oynatıcının C çalışma zamanı DLL'i denetlenmiyordu | `wish_control::pe::needs_c_runtime_dll`. `project_export` uyarır (`export_needs_c_runtime`), `wen package` oynatıcıyı da denetler |
| 5 | `create_from_template` ve `create_project` var olan dosyaların üzerine yazıyordu | Yazılacak dosyalardan biri varsa hiçbir şey yazılmaz: `project_files_exist` |
| 6 | Karşılama ekranı derlenemeyen kodu başarı sayıyordu (`ok`'a bakıyordu) | `passed`'a bakar, derleyicinin ilk hatasını gösterir |
| 7 | Dışa aktarılan kod, projenin çalıştırdığı kod olmayabiliyordu | Diskteki `game.wasm`'ın parmak izi yüklü modülünkiyle aynı olmalı ve kaynaklar ondan yeni olmamalı; yoksa `export_needs_build`. Paket, doğrulanan baytları yazar |
| 8 | Yarıda kalan oluşturma yarım klasör bırakıyordu | Yan klasörde (`.<ad>.creating`) kurulur, tamamlanınca taşınır; hata olursa silinir ve önerilen ad ilerler |
| 9 | Windows'un aygıt adları (`con`, `nul`, `com1`...) kabul ediliyordu | `scaffold::reserved_on_windows`: dışa aktarma adı reddedilir, otomatik adlara `_game` / `-game` eklenir |
| 10 | Okunamayan `editor.json` boş sayılıp üzerine yazılıyordu | Kenara `.json.broken` olarak kopyalanır; yazma yan dosya + yeniden adlandırma |
| 11 | Şablonun gizli klasörleri (`.git`, `.claude`...) yeni projeye geçiyordu | Gizli girdiler kopyalanmaz |
| 12 | Belgeler klasörü OneDrive'a taşınmışsa yanlış yer öneriliyordu | Windows'ta kabuk klasörü kayıt defterinden okunur (`User Shell Folders\Personal`, değişkenler açılır). Bu makinede `OneDrive\Belgeler` |

## 5. Yayın denetimi (WP5, 17 Kasım 2026)

[VISION.md](../VISION.md) başarı ölçütleri ve M9 çıkış ölçütleri. Her biri için kanıt ve durum. Simgeler:
- ✔ kanıtlandı,
- ◐ kodu hazır, kullanıcı adımı bekliyor,
- ✗ yapılmadı.

| Ölçüt | Kanıt | Durum |
|---|---|---|
| Sahne değişikliği → gözlem < 2 sn | Dosya değişince dünya < 1 sn (`wen watch` 100 ms yoklar, M2). Editör sahneyi 0,4 sn'de, projeyi 2 sn'de bir yoklar | ✔ |
| Oyun kodu değişikliği → çalışan oyun < 10 sn | `wen build` düzenleme sonrası 0,4 sn; sıcak yükleme durumu korur (M5 §18) | ✔ |
| 10.000 tick tekrarda sıfır hash sapması | `script_tools.rs::a_ten_thousand_tick_replay_…` (M5); fizikli tekrar (M6) | ✔ |
| Ajan değerlendirme takımı ≥ %90 (Sonnet 5.5 · medium) | 41/41 (%100), `evals/results/m7-sonnet-5-5-medium.json` | ✔ |
| Sıfırdan oynanabilir mini oyun, tek ajan oturumu | Değerlendirme görevleri 11–12 ve 26–41 (oyun kodu yazan, gizli senaryo testiyle denetlenen); vitrin oyunu yalnızca araçlarla kuruldu (M7) | ✔ |
| Yapay zekâsız kullanım eksiksiz | Editör: sahne kurma, oynatma, test, dışa aktarma. Hepsi `wish_editor/tests` altında elle yapılıyor; karşılama ekranı "Kimse" seçeneği | ✔ |
| Elle/ajan eşliği (otomatik denetlenir) | `wish_editor::parity::PARITY` + `tests/parity.rs`: her araç ve her `edit_apply` işlemi elle yapan bir teste bağlı | ✔ |
| İlk çalıştırma: indir → yapay zekâyı bağla → ilk istek → çalışan sahne < 5 dk | Karşılama ekranı → şablon → derleme → editör ~10 sn (`welcome.rs` testi, şablon derlemesiyle). Gerçek Claude Code ile ilk istek kullanıcı onayı ister (kota) | ◐ |
| Kurulum: tek paket, derleyici kurulmaz | `wen package`: `wen` + oynatıcı + SDK + paketlenmiş derleyici + şablonlar; boş ortamda derleme (M5). Temiz Windows Sandbox denemesi kullanıcıda | ◐ |
| Ajan kaynaklı çökme/donma sıfır | Düşmanca oyun modülleri (`wish_script/tests/hostile_game.rs`), bulanıklaştırma (`wish_control/tests/fuzz.rs`), yakıt sınırı; M4–M9 kapanış incelemeleri | ✔ |
| Geri alınabilirlik: her ajan değişikliği tek tıkla | Etkinlik kartı başına geri al; seçmeli geri al (`edit_history revert`); denetim noktaları (M8b) | ✔ |
| Dışa aktarma: tek komut/tık, bağımsız paket | `project_export` / `wen export` / editör menüsü. Dışa aktarılan vitrin oyunu ayrı süreçte projeyle bit bit aynı (`wish_player/tests/export.rs`) | ✔ |
| Dışa aktarılan oyun motorsuz makinede çalışır | Paket motor dosyası içermez (test). Temiz makine denemesi kullanıcıda | ◐ |
| Temiz Windows 10 ve 11'de < 5 dk, ek kurulum yok | Kullanıcı adımı (Windows Sandbox: `clean-test.wsb`) | ◐ |
| Bilinen çökme sıfır, açık yüksek öncelikli hata sıfır | Her kilometre taşı kapanış incelemesinin bulguları kapatıldı (M9: 12/12). Paket denemesinin bulduğu göreli yol hatası düzeltildi. Açık hata kaydı yok | ✔ |
| İsteğe bağlı çökme raporu | Yalnızca yerel: `wish_control::crash` (yakalanmayan panik → rapor dosyası; hiçbir şey gönderilmez); editör bir kez söyler | ✔ |
| Depo herkese açık, katkı kılavuzu yazılmış | `CONTRIBUTING.md` yazıldı. Depoyu açmak kullanıcı kararı | ◐ |
| Ad, marka ve alan adı uygunluğu | Kullanıcı (hukuki/para) | ✗ |
| Son kullanıcı sözleşmesi, Steam sayfası, web sitesi | Taslaklar: `docs/release/` (sözleşme TR/EN, Steam metinleri TR/EN, tek sayfa site). Hukuki inceleme, hesap, alan adı ve yayın kullanıcıda | ◐ |
| Güncelleyici (web) | Bir indirme adresi (barındırma, alan adı) ister; adres belli olunca "yeni sürüm var" denetimi eklenir. O zamana kadar kılavuz ve sözleşme "kendiliğinden indirmez" der | ✗ |

**Kullanıcıya ait adımlar (1.0 öncesi), önerilen sırayla:**
1. ~~Paketi yeniden üret~~: yapıldı (aşağıda "Paket ve uçtan uca deneme").
2. **Temiz makine denemesi.** Windows Sandbox'ta `clean-test.wsb`. Ayrıca bir dışa aktarılan oyunu motorsuz açmak.
3. **Gerçek Claude Code ile kokpit koşumu ve ilk çalıştırma süresi.** Kota harcar.
4. **3 kişiyle kullanıcı testi** (M8b ölçüt 1) ve T1 maketleri.
5. **Ses çıkışını kulakla dinlemek** (`wen play`).
6. **CI faturalandırma.** GitHub işleri başlatmıyor.
7. **Ad/marka/alan adı, son kullanıcı sözleşmesi, depoyu herkese açmak, Steam, web sitesi, kod imzalama.**

### WP5 — paket, uçtan uca deneme, çökme raporu, taslaklar (17 Kasım 2026)

- **Paket:** `C:\wish-build\package` (558 MB), `wen` ve `wish_player` statik C çalışma zamanıyla. Uyarı yok. İçindekiler: SDK, paketlenmiş derleyici, 5 şablon, `clean-test.wsb`.
  - Yeniden üretmek için iki adım:
    1. `RUSTFLAGS=-C target-feature=+crt-static cargo build --release -p wish_cli -p wish_player --target x86_64-pc-windows-msvc --target-dir C:/wish-build/target-static`
    2. O `wen.exe` ile `package --out C:/wish-build/package`.
- **Uçtan uca deneme** (`env -i` ile boş ortam: PATH'te yalnızca `system32`, geliştirme aracı yok):
  - `wen new --template hop` → `wen build` → `wen test` (2/2) → `wen export`.
  - Dışa aktarılan `hop_copy.exe --headless --frames 300` kendi klasöründe çalışır.
  - Dünya hash'i projeyle aynıdır (`1670b0d3…`).
- **Denemenin bulduğu hata (M5'ten beri):** `wen build --root game` gibi göreli bir proje yolunda derleme sessizce başarısızdı.
  - Neden: derleyici proje klasöründe çalışıyor, ona verilen çıktı yolu ise göreliydi; modül `game/game/.wish/...` altına yazılıyordu.
  - Düzeltme: `build_game_within` kökü mutlak yapar. İleti artık boş kalmaz ("exited with ... and wrote no module").
  - Test: `wish_script/tests/build_game.rs::a_project_given_by_a_relative_path_builds`.
- **Çökme raporu** (`wish_control::crash`):
  - Panik kancası son paniği (ileti, yer, geri iz) not eder.
  - `guard` yalnızca `main`'den kaçan paniği rapora yazar. Yakalanan panik (düşmanca model dosyası, başarısız araç) rapor bırakmaz.
  - Yerler:
    - `wen`: `<yapılandırma>/crashes/crash-<zaman>-wen.txt`;
    - oynatıcı: oyunun yanında (oyuncu motoru tanımayabilir, dosyayı oyunu yapana gönderir).
  - Hiçbir şey gönderilmez.
  - Editör yeni raporu bir kez söyler (`crash_notice`, tercihlerde `crash_seen`).
  - Yapılandırma klasörü tek yerde: `wish_control::crash::config_folder` (editör tercihleri de onu kullanır).
  - Testler: `wish_control/tests/crash.rs`, `welcome.rs::a_crash_report_is_told_once`.
- **Taslaklar** (`docs/release/`, yayımlanmadı): son kullanıcı sözleşmesi, Steam metinleri, tek sayfa site.

### WP5 incelemesi (17 Kasım 2026)

Bağımsız inceleme 7 bulgu verdi; hepsi düzeltildi. Testler:
- `wish_control/tests/crash.rs`,
- `wish_script/tests/build_game.rs`.

| # | Bulgu | Düzeltme |
|---|---|---|
| 1 (orta) | Oynatıcı yazılamayan bir klasördeyse (Program Files) rapor ve hata dosyası sessizce kayboluyordu; pencereli sürümde konsol da yok | Sırayla denenir: programın yanı, `%LOCALAPPDATA%\<oyun>`, geçici klasör. Pencereli sürüm hatayı Not Defteri'nde açar |
| 2 (orta) | Yan iş parçacıklarındaki paniklerden rapor çıkmıyordu; MCP giriş okuyucusunun paniği temiz çıkış sayılıyordu | `crash::thread` sarmalı (Claude Code okuyucusu, MCP HTTP kabul ve bağlantı iş parçacıkları, stdin okuyucusu). Okuyucu paniği artık hatadır |
| 3 | Rapor, başka bir iş parçacığının son paniğini taşıyabiliyordu | Notlar iş parçacığı başına tutulur; yalnızca aynı iş parçacığının aynı iletili notu alınır |
| 4 | Yakalanan her panikte geri iz sembolleştiriliyordu (yavaş; kancada çift panik riski) | Geri iz yakalanır ama yalnızca rapor yazılırken çözülür |
| 5 | Çökme bildirimi dil ayarlanmadan önce çevriliyordu | Dil (ve `--lang`) ayarlandıktan sonra |
| 6 | Raporlar dolaşan `%APPDATA%`'ya gidiyordu; aynı saniyede üzerine yazma; sınırsız birikme | `%LOCALAPPDATA%\Wish\crashes`; adda süreç numarası; en yeni 20 rapor tutulur |
| 7 | Göreli yol testi farklı sürücüde yanlış çalışırdı | Sürücüler farklıysa test atlanır |
