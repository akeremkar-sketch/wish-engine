# M8a — Editör temeli (elle düzenleme)

Durum: tasarım (16 Kasım 2026). Dal: `m8a-editor`. İlgili kararlar: ADR-018 (hibrit arayüz), ADR-009 (tek kontrol düzlemi), ADR-003 (şema önce), değişmezler 4, 6, 8, 9. Önceki: M7 bitti; editörün dayanacağı her şey (kontrol düzlemi, oturumlar, `describe_view`, geri al/yinele, pencere) vardır.

## 1. Hedef ve sınır

Hiçbir yapay zekâ bağlı değilken bir kişi vitrin oyununun (`examples/islands`) bir bölümünü **yalnızca elle** kurar, oynatır, hata ayıklar ve dışa aktarıma hazır hâle getirir. Editör kontrol düzleminin **ince bir yüzüdür**: arayüzdeki her eylem bir `Workspace::call` çağrısıdır; arayüzde olup araçta olmayan yetenek olamaz (değişmez 4). Kokpit (konuşma, "göster ve söyle", etkinlik akışı) M8b'dedir; M8a yalnızca onun oturacağı yeri ve etkinlik akışının veri yolunu (`actor`) hazırlar.

**Çıkış ölçütleri (ROADMAP):**
1. Yapay zekâsız, yalnızca elle: vitrin oyununun bir bölümü kurulur, oynanır, dışa aktarıma hazırlanır.
2. Eşlik denetimi: kontrol düzlemindeki her sahne/varlık işleminin arayüzde elle karşılığı vardır (otomatik liste, test).
3. Arayüzdeki her eylem bir kontrol düzlemi işlemidir; geri al/yinele her eylemde çalışır.
4. Wasm'da yeni tanımlanan bir bileşen, ek arayüz kodu yazılmadan denetçide düzenlenebilir.

## 2. Karar: arayüz teknolojisi (ADR-018 "geçici kısım" kesinleşiyor)

**Karar: süreç içi `egui` 0.37 (`egui` + `epaint`, MIT OR Apache-2.0), kendi CPU rasterleyicimizle; GPU'ya dokunmaz, mevcut `wish_window` yolunu izler.**

Seçenekler:

| Seçenek | Artı | Eksi |
|---|---|---|
| A. `egui-wgpu` (GPU'da çiz) | Standart yol, hızlı | Değişmez 6'yı zorlar (wgpu yalnızca `wish_render`'da): ya `wish_render` egui'yi tanır ya editör wgpu'ya dokunur; pencere yüzeyi/aygıt yönetimi M3 WP8'in "ekran dışı çiz + softbuffer" kararını bozar; başsız test zor. |
| B. `egui` tesselasyonu + **kendi CPU rasterleyicisi** (seçilen) | Pencere yolu aynı kalır (renderer ekran dışına, UI üstüne CPU'da biner, `softbuffer` gösterir); **başsız**: UI de bir resme çizilir, altın görüntülerle test edilir (değişmez 8); belirlenimci; yeni yerel bağımlılık yok | Rasterleyiciyi biz yazarız (~400 satır: dokulu renkli üçgen, kırpma, alfa); büyük pencerede CPU maliyeti (WP1'de ölçülür) |
| C. `egui_software_backend` 0.0.3 | Hazır | Olgun değil (6,7 bin indirme, 0.0.x); belirlenimcilik/test garantisi yok |
| D. HTTP + web arayüzü (webview) | Zengin metin güçlü | Görünüm penceresini gömmek zor; webview bağımlılığı; kokpit tasarımı hâlâ yok |

Gerekçe: B hem mimariyi (değişmez 6) hem sınanabilirliği (değişmez 8) korur ve motorun geri kalanı etkilenmez; teknoloji sonradan değişirse editör kabuğu değişir, çekirdek değişmez. **Risk:** CPU maliyeti. Önlem: WP1 kanıt işi 1920×1080'de kare süresini ölçer; bütçe kare başına ≤ 6 ms (UI) — aşılırsa seçenek A'ya dönüş yolu `wish_render`'a dar bir "bindirme" uzantısıyla açıktır (tasarım notuna ADR güncellemesiyle).

Bu karar `docs/DECISIONS.md` ADR-018'e işlenir (WP1'de). Bağımlılık lisansları `deny.toml` izinli listesinde (MIT/Apache-2.0); eklenince `cargo metadata` ile denetlenir.

## 3. Mimari

Yeni crate'ler (bağımlılık yönü aşağı; editör kontrol düzlemini ve pencere/renderı kullanır, onlar editörü tanımaz):

- **`wish_editor`** — kütüphane: `EditorState` (seçim, açık paneller, kamera, düzen), `Panel` özelliği, her panel `fn ui(&mut self, ctx, &mut Workspace)`; hiçbir panel sahne dosyasına ya da dünyaya doğrudan yazmaz: yalnızca `Workspace::call` (araç adı + argüman). Başsız kullanım: `Editor::run_frame(input) -> Frame` (egui çıktısı + tesselasyon), `render_frame(&Frame) -> RgbaImage` (CPU rasterleyici). Testler girdiyi (fare/klavye olayları) enjekte edip kareyi okur.
- **`wish_editor_raster`** (ya da `wish_editor` içinde `raster` modülü): epaint ağını resme çizen belirlenimci rasterleyici.
- **`wen edit [--root DIR]`** — pencereli editörü açar (`wish_window` genişletilir: renderer görünümü + UI bindirmesi + girdi iletimi). `wen edit --headless-frame <girdi.json> --out kare.png` başsız yakalama (test ve inceleme).

Veri akışı: UI olayı → panel → **tek** `Workspace::call(tool, args, actor = "human")` → sonuç/tanı UI'ya; sahne değişimi `edit_apply` ile geçmişe girer (aynı geri al yığını); dış/ajan değişikliği `changed_by_others` ile gelir ve paneller yenilenir. Görünüm: sahne `capture_frame`/render yoluyla çizilir (aynı `Renderer`), **oynatırken** oturum kareleri.

**Şemadan otomatik denetçi:** `schema_get` her bileşen için alanları, türü, birimi, aralığı, belge yorumunu verir; denetçi bu veriden widget üretir (sayı → sürükle kutusu birim ve aralıkla, `bool` → onay kutusu, `enum` → açılır liste, `Vec3`/`Quat`/`Color` → özel widget'lar, varlık/varlık başvurusu → seçici, `AssetRef` → varlık seçici). Wasm'da tanımlanan bileşen de `schema_get`'te göründüğünden **ek kod yazmadan** düzenlenir (ölçüt 4).

**Eşlik denetimi (ölçüt 2):** her araç/işlem için UI'daki karşılığın adını bildiren tek tablo (`wish_editor::parity::TABLE`: araç adı → panel/eylem kimliği); test, `wish_control`'ün araç listesindeki her sahne/varlık işleminin tabloda olduğunu ve her tablo girdisinin gerçek bir eylem kimliğine bağlandığını doğrular (yeni araç eklenirse test kırılır, M4 eşlik testiyle aynı kural).

## 4. Kapsam ve iş paketleri

Önerilen model · efor, ROADMAP'e göre Sonnet 5.5 · high; etkileşim/tasarım kararlarında Opus 5.5 · high.

| WP | İş | Model · efor |
|---|---|---|
| WP1 | **Kanıt işi:** egui başsız (tesselasyon) + CPU rasterleyici; altın görüntü; 1920×1080 kare süresi ölçümü; ADR-018 güncellemesi; `wish_editor` iskeleti, `wen edit --headless-frame` | Opus 5.5 · high |
| WP2 | Kabuk: pencere (UI bindirmesi + görünüm), düzen, kamera gezinmesi (`FlyCamera`), proje aç/kaydet, `changed_by_others`, `wen edit` | Sonnet 5.5 · high |
| WP3 | Hiyerarşi (`scene_get`, `entity_query`; ekle/sil/yeniden adlandır/yeniden ebeveynle, sürükle-bırak) + seçim (görünümde tıklayarak: `IdBuffer`) | Sonnet 5.5 · high |
| WP4 | Denetçi: şemadan üretilen widget'lar, birim/aralık, bileşen ekle/kaldır, çok seçim, prefab geçersiz kılmalar; geri al/yinele (`edit_history`) | Opus 5.5 · high (şema→widget tasarımı), Sonnet 5.5 · high |
| WP5 | Tutamaçlar: taşı/döndür/ölçekle (yerel/dünya, yapışma); sürükleme bir `edit_apply` (bırakınca tek işlem, sürerken önizleme) | Sonnet 5.5 · high |
| WP6 | Varlık tarayıcı (`asset_list/info/move`, küçük resimler, sürükle-bırak sahneye/denetçiye), materyal düzenleyici, prefab düzenleme (M4'ten devir: prefab düzenleme araçlaşır) | Sonnet 5.5 · high |
| WP7 | Oynatma: oynat/duraklat/adım/geri sar, `sim_snapshot` zaman çizelgesi, **`edit_apply`'ın canlı oturuma uygulanması** (M5'ten devir), girdi haritası düzenleyici (`input.json`) | Opus 5.5 · high |
| WP8 | Konsol ve tanılar (`logs_get`, `check`), performans paneli (`profile_get`), proje ayarları, "dış düzenleyicide aç", Türkçe/İngilizce, **Streamable HTTP taşıması** (M4'ten devir; editör ve ajan aynı süreçte), dışa aktarım hazırlığı denetimi | Sonnet 5.5 · high |
| WP9 | Eşlik denetimi tablosu ve testi, çıkış senaryosu "yalnızca elle", bağımsız kapanış incelemesi, M8a sonucu, birleştirme | Opus 5.5 · high |

## 5. Test planı (değişmez 8: başsız)

- **Altın görüntü:** her panel ve düzen için `Editor::run_frame` + CPU rasterleyici ile altın PNG'ler (WARP/yazılım tutarsızlığı yok: UI saf CPU, bit düzeyinde aynı olması hedeflenir; görünüm bölgesi hariç tutulur ya da renderer altın toleransıyla ayrı).
- **Girdi testleri:** fare/klavye enjekte edilir ("hiyerarşide `crate`'e tıkla, denetçide `Transform.translation.y`'yi 2'ye yaz"), sonra `entity_get` ile kontrol düzlemi değeri doğrulanır; geri al ile eski değere dönüş.
- **Sürükleme = tek işlem:** tutamaç sürüklemesi geçmişte **bir** kayıt bırakır; iptal (Esc) hiç kayıt bırakmaz.
- **Eşlik testi:** §3. **Şema testi:** `schema_get`'teki her `Plain` türünün widget'ı üretilebilir (bilinmeyen tür → "ham JSON" yedek widget ve uyarı; test, motor türlerinin hiçbirinin yedeğe düşmediğini doğrular).
- **Elle uçtan uca:** `tests/editor_manual.test.json` benzeri betik (kaydedilmiş girdi akışı): yalnızca UI girdisiyle bir bölüm kurulur, oynatılır, `test_run` geçer. Pencere döngüsü CI'da çalışmaz (yerelde `wen edit --exit-after`), ama başsız kare yolu CI'dadır.

## 6. Riskler

| Risk | Etki | Önlem |
|---|---|---|
| CPU rasterleyici yavaş | Editör hantal | WP1'de ölçüm; bütçe 6 ms; kirli bölge çizimi; en kötü hâlde seçenek A'ya dar dönüş |
| T1 maketleri hiç yapılmadı | Düzen/akış kullanışsız çıkabilir | WP2 sonunda başsız yakalanan gerçek düzen kullanıcıya gösterilir (maket yerine çalışan iskelet); itirazlar WP3'ten önce işlenir. Kokpit maketleri (M8b ön koşulu) ayrı kalır. |
| Denetçi şema çeşitliliğinde yetersiz kalır | Ek kod gerekir (ölçüt 4 kırılır) | Yedek ham-JSON widget'ı; tür başına özel widget yalnızca `Vec3/Quat/Color/AssetRef/Entity` |
| Elle ve ajan değişikliği çakışması | Kayıp iş | Mevcut `base_revision` + `changed_by_others`; editör her yazımda `base_revision` gönderir |
| Kapsam şişmesi (uzman araçlar) | 1.0 gecikir | Animasyon zaman çizelgesi, gölgelendirici grafiği, arazi, boyama **yok** (ADR-018) |

## 7. Kullanıcıya ait kararlar

Yok (teknoloji kararı §2'de gerekçeli). Kullanıcıdan istenecek tek şey, WP2 sonunda çalışan iskeletin düzenine bakıp itiraz etmesi (T1 yerine).

## 8. Uygulama notları

### WP1 — kanıt işi: başsız egui + CPU rasterleyici

- **Crate `wish_editor`** (bağımlılıklar: `egui` 0.37 varsayılan özellikler kapalı, `wish_core`, `wish_text` yalnızca Roboto baytları için). Lisans taraması (`cargo metadata`, 70+ paket): hepsi MIT/Apache-2.0/BSD/Zlib/BSL/Unicode, izinli listede; varsayılan yazı tipi paketleri (OFL/Ubuntu) kapalı olduğundan girmez.
- **`raster`**: `Canvas` (RGBA8, sRGB, önceden çarpılmış alfa), `Textures` (egui doku farkları, sıralı uygulanır), `paint` (üçgenler). Köşeler 1/256 piksele yuvarlanır, kapsama **tam sayı** kenar fonksiyonları ve üst-sol kuralıyla belirlenir: ortak kenarda çift boyama ya da boşluk olmaz (yarı saydam şekillerde dikiş yok); doğrusal/yakın doku örnekleme; "düz" üçgenler (tek renk, tek doku noktası) enterpolasyonsuz hızlı yoldan çizilir. Ağ kayan nokta işlemleri `+ - * /` ve yuvarlama, makineye bağlı işlev (sin/exp) yok.
- **`Editor`**: `layout` (egui koşar, girdi olayları verilir, saat yok: süre çağırandan gelir) → `Frame` (üçgenler), `paint(frame, canvas)`, `run_frame` ikisi birden. Yazı tipi: yalnızca Roboto, iki ailede.
- **Testler (`tests/raster.rs`):** altın görüntü (`inspector.png`: başlık, sürükle kutusu, onay kutusu, metin alanı, kaydırıcı, düğme; **bayt bayt eşit**), iki editör aynı resmi çizer, bir fare tıklaması onay kutusunu değiştirir, kare süresi ölçümü (`--ignored`, release).
- **Ölçüm (release, 1920×1080, 60 etiket + 8 denetçi paneli, 6.524 üçgen):** düzen 0,6 ms, çizim 4,3 ms (ilk sürüm 11,7 ms'ydi; düz üçgen yolu ve artımlı kenar fonksiyonları 2,7 kat kazandırdı), tuval ayırma 2,4 ms (gerçekte yeniden kullanılır). Bütçe (düzen + çizim ≤ 6 ms) tutuyor; kirli bölge çizimi gerekirse WP2'de.
- **egui 0.37 notları:** `SidePanel` → `Panel::left/right(..).show(ui, ..)`; `Context::run_ui`; uygulanan `TexturesDelta` `clear()` ile boşaltılmalı (debug'da `Drop` denetler).

### WP2 — kabuk: pencere, görünüm, `wen edit`

- **`wish_control::Workspace::render_picture(kaynak, kamera, w, h, görünüm) -> Picture`** (+ `PictureSource`, `Picture`): `capture_frame` ile **aynı** hazırlık ve çizim yolu; PNG yerine resmin kendisini, kamerayı birinci kameranın bir işlevi olarak alır (uçuş kamerası). Editörün görünümü bununla çizilir; `prepare`/`drawing` argümansız çekirdeklere ayrıldı (araçlar değişmedi).
- **`wish_window::run_shell`** (`shell.rs`): egui'yi bilmeyen genel kabuk döngüsü. Düz `ShellEvent`'ler (işaretçi, düğme, tekerlek, tuş adı egui'nin yazımıyla, metin, ham fare farkı) gelir; `Shell::frame(boyut, süre, olaylar) -> Option<Picture>` hazır RGBA döner (`None`: değişmedi); pencere `softbuffer` ile gösterir. 16 ms'de bir ve girdi sonrası çağrılır.
- **`wish_editor::EditorApp`** (`app.rs`, bir `Shell`): üst menü (proje, sahne seçici, Geri al/Yinele, görünüm kipi, kamerayı sıfırla, Kontrol, Çık), sol hiyerarşi (şimdilik salt okunur; WP3), sağ denetçi (yer tutucu; WP4), alt durum çubuğu (hata/uyarı sayısı + son ileti), ortada egui'nin **boyamadığı** orta alan: sahnenin resmi oradan görünür (arka plan → sahne → paneller sırasıyla çizilir). Sağ düğme basılıyken görünümde fare = bakma, W A S D E Q uçuş, Shift hızlı.
- **Her eylem bir kontrol düzlemi çağrısıdır, aktör `human`** (`EditorApp::call`): Geri al/Yinele `edit_history`, Kontrol `check`; okumalar `project_info`/`scene_get`/`edit_history`. **Başkasının değişikliği:** her 0,4 sn `scene_get` revizyonuna bakılır; ajan ya da elle dosya düzenlemesi hiyerarşiye ve resme yansır (M4'ün `changed_by_others` mekanizmasıyla aynı tek geçmiş).
- **Başsız:** `wish_editor::script` (girdi betiği: `pointer`, `press/release`, `click`, `key`, `text`, `scroll`, `mouse_delta`; adımlar `events`/`idle`/`seconds`), `wen edit --headless-frame betik.json --out kare.png [--software]`. Pencereli: `wen edit [--root K] [--size WxH] [--software] [--exit-after N]` (yerelde denendi: açılır, 5 kare, kapanır; CI'da pencere yok).
- **Testler (`wish_editor/tests/app.rs`, release ve debug'da geçer):** hiyerarşi + sahne resmi, başkasının değişikliği (kontrol düzlemi *ve* dosyayı elle düzenleme) hiyerarşiye/resme yansır, Geri al düğmesi `human` imzalı tek geçmişe girer, sağ sürükleme/tuşlarla kamera hareketi, kötü betik tanıdır, **tüm pencerenin altın görüntüsü** (arayüz bayt bayt, sahne bölgesi ≤3/255).
- **Bilinen sınırlar (WP3+):** hiyerarşi tıklanamaz, görünümde seçim yok, denetçi boş; kirli bölge çizimi yok (her değişimde tam kare, tam HD'de ~5 ms + sahne). Düzen kullanıcıya gösterilmek üzere: [M8a-wp2-editor-window.png](M8a-wp2-editor-window.png).

### WP3 — hiyerarşi ve seçim

- **Seçim modeli:** `EditorApp::selection()` (sıralı; ilki "ana"), `select(&[ids])`; hiyerarşide tık = seç, **Ctrl+tık** = seçime ekle/çıkar, boş alana tık = seçimi kaldır; görünümde tık = o pikseldeki varlığı seç (Ctrl: ekle/çıkar), boşa tık = temizle. Seçim sahneye dokunmaz: geçmişe girmez. Başkası bir varlığı silip yeniden adlandırınca seçim ona göre budanır.
- **Hiyerarşi eylemleri (hepsi tek `edit_apply`, aktör `human`, `base_revision` ile):** "+" ekle (`entity_N`, `Transform`lu; sağ tık "Add child" altına), "Delete" ya da **Delete tuşu** (seçili hepsi tek çağrıda), **çift tık / F2 / sağ tık "Rename"** satır içi metin kutusu (`rename` işlemi; Enter uygular, Esc/odak kaybı vazgeçer; seçim yeni ada taşınır), **sürükle-bırak** başka satıra = çocuk yap, panelin boş alanına = köke al (`reparent`; kendi üstüne bırakma yok sayılır, döngü motorun tanısıyla reddedilir ve durum çubuğunda görünür). Her eylem geçmişte **tek** kayıt; Geri al tek adımda geri alır.
- **Çakışma:** düzenleme, kişinin gördüğü revizyonla gönderilir (`base_revision`); bu arada başkası değiştirdiyse çağrı reddedilir, kişiye "failed: ..." denir, hiyerarşi yenilenir — kişi ajanın işini ezmez (testli).
- **Görünümde seçim:** `Workspace::render_ids` (`IdPicture`: `IdBuffer` + isimler; `render_picture`'la aynı kamera); seçili varlığın ekran kutusu kimlik tamponundan (2 pikselde bir tarama) çıkarılır ve görünümün üstüne **turuncu 2 piksellik çerçeve** çizilir (`describe_view`'e değil, uçuş kamerasına bağlı olduğu için).
- **Kabuk iyileştirmeleri:** eylem ya da seçim sonrası **ikinci düzen geçişi** (panel, yaptığın şeyi aynı karede söyler: Delete düğmesi, denetçi başlığı); `ShellEvent::Modifiers` → egui `ModifiersChanged` (Ctrl tutma durumu; betikte `{ "modifiers": { "ctrl": true } }`); `wen edit` betiğinde tıklamalar aynı yolu kullanır.
- **Testler (`wish_editor/tests/hierarchy.rs`, release+debug):** tık/Ctrl-tık seçimi ve boş alan, ekle/sil/geri al (3 kayıt, hepsi `human`), Delete tuşu, çift tıkla yeniden adlandırma, sürükle-bırak ile ebeveyn değiştirme ve köke alma, görünümde tıklayarak seçme + çerçeve + gökyüzüne tıklayınca temizleme, bayat revizyon ajanın işini ezmez. Altın görüntü yenilendi (yeni düğmeler).
- **Sınır (WP4+):** denetçi hâlâ boş (yalnızca seçili adı yazar); çok seçimde ortak düzenleme WP4'te.
