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
