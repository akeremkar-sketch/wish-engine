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
| WP8 | Konsol ve tanılar (`logs_get`, `check`), performans paneli (`profile_get`), proje ayarları, "dış düzenleyicide aç", **çok dilli arayüz** ("bir dil = bir dosya": kodda sabit metin yok, eksik çeviri İngilizceye düşer ve testte listelenir; 1.0'da Latin/Kiril/Yunan alfabeli 10–12 dil, Roboto ile; CJK/Arapça/Hintçe M10 — kullanıcı kararı 16 Kasım 2026), görsel cila, **Streamable HTTP taşıması** (M4'ten devir; editör ve ajan aynı süreçte), dışa aktarım hazırlığı denetimi | Sonnet 5.5 · high |
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

### WP4 — denetçi (şemadan, ek kod yok)

- **`wish_editor::inspector`:** seçimin bileşenleri `entity_get` ile (etkin değerler) okunur, widget'lar `schema_get` (`detail: full`) JSON Schema'sından **genel** kurallarla üretilir: sayı → birim + aralıklı sürükle kutusu (tam sayıda ondalıksız), bool → onay kutusu, `oneOf` → açılır liste (her seçeneğin belgesi ipucunda), sabit uzunluklu sayı/bool dizisi → x/y/z/w kutuları (`Color`: + renk seçici, `Quat`: **derece** — `edit_apply`'ın `euler_deg` sırasıyla, Y sonra X sonra Z), `Entity` → sahnedeki varlıklar listesi, `AssetRef` → metin + proje dosyaları listesi, `Text` → metin kutusu, iç içe nesne → özyinelemeli ızgara, nesne dizisi → katlanmış özyineleme, gerisi → ham JSON (uyarıyla). Alan adının ipucu belge yorumu + birimdir. **Testle: motorun 59 türünün hiçbir alanı ham JSON'a düşmez** (`has_widget`).
- **Ne zaman gönderilir:** sürükleme bırakılınca (bu sırada yalnızca denetçide önizleme; **tek geçmiş kaydı**), yazma Enter'da/odak kaybında, tık hemen. Hepsi `set` (seçili her varlık için, **tek** `edit_apply`, `human`, `base_revision`); bileşen ekle (aranabilir menü: yazınca süzülür) = `set {}` (varsayılanlar), kaldır = `remove`. Çoklu seçim: ortak bileşenler, ilkinin değerleri gösterilir, değişiklik hepsine.
- **Prefab parçası** (`örnek/parça`, görünümden seçilir): değişiklik `set_override` olur; prefab dosyası değişmez; ekle/kaldır yok; örnek varken seçimde kalır. (Prefab dosyasının kendisini düzenlemek WP6.)
- **Wasm bileşeni (çıkış ölçütü 4):** editör 2 sn'de bir `project_info`'ya bakar; oyun kodunun parmak izi değişince şemaları yeniden okur. Test: editör açıkken `Spin { speed: f32 (deg/s), backwards: bool }` tanımlayan oyun kodu derlenir → "Add component"te çıkar → eklenir → `speed` birimiyle yazılır, `backwards` tıklanır — **editörde bu bileşen için tek satır kod yok**.
- **Kabuk:** egui'nin "hemen yeniden çiz" isteğine uyulur (açılır menüler bir sonraki karede açılır; önceden girdi gelmeden açılmıyordu — gerçek pencerede de hataydı). **Erişilebilirlik ağacı** (AccessKit, egui'de hazır) açıldı: `Editor::widgets()` / `EditorApp::widgets()` her widget'ın rolü, etiketi, değeri ve yeri; testler widget'ları **adıyla** bulur (piksele bağlı değil), ileride ekran okuyucu ve ajanın arayüzü "görmesi" için de temel.
- **Testler (`wish_editor/tests/inspector.rs`, 8):** her alanın widget'ı var; sayı yazmak tek `human` çağrısı; sürükleme bırakılana dek gönderilmez ve tek kayıt; açılır liste + bileşen ekle/onay kutusu/kaldır; çoklu seçim tek çağrı; döndürme derece; Wasm bileşeni; prefab parçası geçersiz kılma. Editörde toplam 24 test, release+debug.

### WP5 — tutamaçlar (taşı / döndür / ölçekle)

- **`wish_editor::gizmo`** (saf matematik, pencere ve sahne yok, 8 birim testi): `Eye` (kameranın görünümü: `project` dünya→pencere, `ray` pencere→ışın; perspektif ve ortografik), **sabit ekran boyu** (110 px, uzaklıktan bağımsız), `hit_test` (en yakın tutamaç, 9 px içinde), `Drag` (başla/ölç/dönüştür). Sürükleme **ışından** okunur, bu yüzden işaretçi her mesafe ve açıda tam izlenir: taşı = eksen çizgisinin ışına en yakın noktası; döndür = halkanın düzleminde ışının vurduğu noktanın açısı (en kısa yol, sağ el kuralı); ölçekle = eksen üzerindeki mesafe oranı, merkezde işaretçinin merkeze uzaklık oranı (sıfıra bölme korumalı).
- **Üst nesne ve eksenler:** sürükleme dünya uzayında hesaplanır, `Transform` yerel olduğundan **ebeveynin dünya matrisinden** geçirilir (`GlobalTransform * yerel⁻¹`; ebeveynli taşıma testli). Eksenler **Dünya/Yerel** (menüde düğme), **Yapışma** (0,5 m, 15°, %10 adım) menüde.
- **Kiplerin seçimi:** menüde Move/Rotate/Scale düğmeleri ve **W/E/R** tuşları (sağ düğme basılıyken uçuş tuşları olarak kalır; yazı yazarken kip değişmez).
- **Sürüklerken önizleme, bırakınca tek işlem:** `PictureSource::Preview(sahne, ops)` — işlemler belge **kopyasında** uygulanıp o hâliyle çizilir (`Workspace::build_preview`; dosya, geçmiş ve revizyon dokunulmaz; testli, `wish_control/tests/m7_tools.rs`). Bırakınca **tek** `edit_apply` (`set Transform {yalnız değişen alan}`, `human`, `base_revision`): geçmişte tek kayıt. **Esc** hepsini geri alır (kayıt yok, durum "cancelled").
- **Görünüm iyileştirmeleri:** tutamaçlar seçili varlığın **ana** olanına (ilk); prefab parçasında yok (yazacağı geçersiz kılma WP6 kapsamı); sürükleme bırakılana dek seçim kutusu yeniden hesaplanmaz; tutamaçlar kamerayla birlikte (son çizilen kamera kaydı) ve sürükleme sırasında önizlemedeki yeri izler.
- **Testler (`wish_editor/tests/gizmo.rs`, 7):** çizim ve sabit uzunluk, X tutamacı sürükle → tek kayıt `human`, bırakılana dek sahne değişmez ama resim değişir, Esc geri alır, yapışma 0,5 m, E tuşu + Z halkası çeyrek tur, R tuşu + merkez tutamağı üç ekseni ölçekler, seçimsiz tutamaç yok. Editörde toplam 32 test, release+debug.

### WP6 — varlık tarayıcı, prefab düzenleme, materyal

- **Kontrol düzlemi (M4'ten devir: prefab düzenleme):** `edit_apply` artık `.prefab.json` dosyalarını da düzenler (reddeden `prefab_read_only` kalktı, katalogdan çıktı); `scene` yolu bir prefab, `create: true` yeni prefab açar. Prefab kendi geçmişine sahiptir (geri al çalışır); yazım sonrası **kütüphane hemen yenilenir** (`prefab_stamp` sıfırlanır: aynı saat diliminde yapılan düzenleme kaçmasın) ve örnekleri olan sahneler yeniden kurulur; örneğin kendi geçersiz kılması prefab düzenlemesinden sonra da kazanır (testli, `m7_tools.rs`). `capture_frame`/`render_picture` bir prefab'ı çizerken kamera ve ışık yoksa **varsayılan sahne** (öndan/yukarıdan kamera + gün ışığı) ekler. `Workspace::render_picture`/`render_ids` için `PictureSource::Preview` WP5'ten.
- **Varlık tarayıcı (`wish_editor::browser`, alt panel, yeniden boyutlanır):** `asset_list` + prefab'lar tek listede; süzgeç (yazı) + tür düğmeleri (Images/Models/Sounds/Prefabs); PNG'ler için **küçük resim** (bir kez çözülür, 8 MB üstü atlanır), diğerleri tür rozeti; seçince `asset_info` ayrıntısı (kimler kullanıyor dahil); **çift tık ya da sağ tık "Add to scene"** (prefab → `instantiate`, model → `Model`'li varlık; görünümün ortasında zemine); **sürükle-bırak**: görünüme bırak = işaretçinin zemin (y=0) noktasına yerleştir (zemini görmüyorsa ışın boyunca 6 m); denetçide bir `AssetRef` kutusuna bırak = değeri ayarlar; **sağ tık "Rename / move..."** = `asset_move` (kullanan sahne ve prefab başvuruları güncellenir, durum çubuğu kaç dosyanın güncellendiğini söyler).
- **Prefab düzenleme:** sahne seçici sahnelerin altında prefab'ları da listeler; bir prefab'ı seçince hiyerarşi/denetçi/tutamaçlar/görünüm sahneyle **aynı** şekilde çalışır (ortak yol), kaydedince örnekler hemen takip eder. **Hiyerarşide sağ tık "Make prefab":** seçili varlık ve çocukları `prefabs/<ad>.prefab.json` olur (kök `root`, kökün Transform'u nötr), sahnede aynı kimlik, ebeveyn ve Transform ile **örneği** yerine konur. İki dosya, iki geçmiş (prefab dosyası + sahne): sahne tek geri almayla eski hâline döner, prefab dosyası kalır. İçinde başka prefab örneği olan varlık reddedilir (durum çubuğu söyler).
- **Materyal (kapsam notu):** bu motorda materyal ayrı bir varlık dosyası değil `Material` bileşenidir; düzenlemesi denetçide (renk seçici, metallic/roughness aralıklı kutular, emissive) zaten var. WP6'da eklenen: bileşen başlığına **sağ tık → Copy values / Paste values / Reset to defaults** (her bileşen için; seçimin hepsine; prefab parçasında geçersiz kılma). Önizleme küresi/materyal kütüphanesi **yapılmadı** (kütüphane bir dosya biçimi gerektirir; M10 listesine).
- **Hata bulundu ve düzeltildi:** periyodik proje yoklaması (2 sn) dosya/prefab/sorun sayısını güncelliyor ama o kare yeniden çizilmiyordu (yeni dosya ancak bir etkileşimden sonra görünürdü); `refresh_project` artık "gösterilen şey değişti mi" döndürür.
- **Testler:** `wish_editor/tests/assets.rs` (9: listele+süz+küçük resim, ayrıntı, çift tık yerleştirme, sürükleyip görünüme bırakma, alana bırakma, taşıma + kullanan güncellenir, Make prefab + geri al, prefab'ı editörde düzenle + örnek takip eder, kopyala/yapıştır/sıfırla); `wish_control/tests/m7_tools.rs` (prefab oluştur/düzenle/örnek/geçersiz kılma/geri al); `scene_tools.rs` (prefab artık düzenlenir). Editörde toplam 41 test, release+debug.

### WP7 — oynatma, zaman çizelgesi, canlı oturumda düzenleme

- **Kontrol düzlemi (M5'ten devir): `edit_apply` + `session`** (`wish_control::live`): işlemler o çalışan oyunun dünyasına uygulanır — dosyaya ve geçmişine değil, oturum sürdükçe kalır. Hep ya da hiç (başarısız olursa anlık görüntüyle geri konur), `dry_run`, aynı `location.op` tanısı. Desteklenen: spawn (ebeveynli), despawn (çocuklarıyla), set, replace, remove, reparent, rename, resource işlemleri; dosyaya özgü olanlar (instantiate, geçersiz kılmalar, notlar) `live_edit_unsupported` + ipucu. **Tekrar kaydı yapan oturum reddeder** (`live_edit_while_recording`: düzenleme girdi değildir, kayıt aynı oynamaz). Her canlı düzenleme oturum günlüğüne `edit` türüyle girer (`logs_get kinds: ["edit"]`, kim neyi değiştirdi). Düzenleme sonrası `GlobalTransform` hemen güncellenir (resim/sorular beklemez). Yeni tanılar katalogda.
- **Editörde oynatma:** menüde **Play** (prefab açıkken kapalı) → `sim_start`; **Pause/Resume**, **Step** (duraklatılmışken bir tick), **Stop** (`sim_stop`, "canlı değişiklikler tutulmadı" der); `tick N` göstergesi. Gerçek zaman 60 Hz tick'e çevrilir (kare başına en çok 8, yavaş karede yarış yok). Görünüm oyunun **kendi kamerasından** çizilir; seçim çerçevesi ve tıklayarak seçim oturum dünyasından.
- **Girdi:** oynarken, işaretçi görünümün üstündeyken tuşlar (oyunun adlarıyla: oklar `Up/Down/Left/Right`), Shift/Ctrl/Alt ve fare düğmeleri (`MouseLeft`...) oyuna gider; işaretçinin yeri `pointer` olarak (resim içi piksel). Editör kısayolları (W/E/R, Delete, F2) oynarken kapalı; yazı alanına yazarken tuşlar oyuna gitmez.
- **Zaman çizelgesi:** her 2 sn oyun zamanında otomatik `sim_snapshot` (`t<tick>`, en çok 30; oturum sınırı 32); menüdeki kaydırıcıyı bırakınca o ana **geri sar** (`sim_restore`, duraklatılmış olarak). Kaydırıcı değeri bırakılana dek bekler (tık da sürükleme de).
- **Oynarken düzenleme:** denetçi oturumun canlı değerlerini gösterir (0,25 sn'de bir okur) ve değişiklikler `edit_apply`+`session` olur (durum çubuğu "live: not kept" der); hiyerarşide ekle/sil de canlı. Tutamaçlar oynarken kapalı (önizleme sahne içindi).
- **Girdi haritası düzenleyici WP8'e taşındı:** `input.json`'u elle düzenlemek bir kontrol düzlemi yeteneği ister (değişmez 4); proje ayarları (`wish.json`) ile birlikte tek bir ayar aracında yapılacak.
- **Testler:** `wish_control/tests/m7_tools.rs` (canlı düzenleme: set/spawn/rename, dünya sürer, dosya ve geçmiş dokunulmaz, günlük, hep-ya-hiç, dry_run, desteklenmeyen işlem, kayıt reddi); `wish_editor/tests/play.rs` (5: oynat → kutu düşer → durdur dosya aynı; duraklat + adım; zaman çizelgesiyle başa dönüş + kutu yerinde; oynarken denetçiden canlı düzenleme, durdurunca yok; Space ve işaretçi oyuna gider). Editörde toplam 46 test, release+debug.

### WP8 — ayarlar, konsol/sorunlar/performans, kod düğmeleri, çok dilli arayüz, cila, Streamable HTTP

- **`project_settings` (26. araç):** projenin adı ve varsayılan sahnesi (`wish.json`) ile girdi haritası (`input.json`) okunur/değiştirilir; ad 1–80 karakter (`invalid_project_name`), sahne var olmalı, girdi haritası motorun kendi ayrıştırıcısıyla denetlenir (yanlış tuş adına en yakın gerçek ad ipucu olur), `dry_run` (`would_change`). Eşlik örnekleri 4. Böylece girdi haritası düzenleyici değişmez 4'e uyar (ajan da aynı aracı kullanır).
- **Editör:** proje menüsü (projenin adı: Ayarlar..., Denetle, Kodu düzenle, Kodu derle, Dil, Çık) — kalabalık çubuk taşıyordu. Ayarlar penceresi (ad, varsayılan sahne, eylemler/türler/tuşlar; ekle/kaldır; kaydet = araç çağrısı). Alt panel sekmeleri: Varlıklar, **Konsol** (`logs_get`, sırayla, Temizle), **Sorunlar** (`check`; tıklayınca sorunun varlığı seçilir; sayı projenin hata sayısını izler), **Performans** (kare başına arayüz/sahne resmi/oyun süreleri; 16,7 ms çizgisi — editörün kendi süresi, simülasyon değil). **Kodu düzenle** `scripts/src/lib.rs`'i dışarıda açar (`WISH_EDITOR` ya da sistemin programı; testte değiştirilebilir `Opener`), **Kodu derle** `script_build` çağırır ve sorunlarını Sorunlar'da gösterir; varlık tarayıcısında "Dışarıda aç".
- **Çok dilli arayüz (`wish_editor::i18n`):** koddaki her metin `t("...")`/`tf("...", adlar)`'dan geçer, anahtar İngilizce metnin kendisi; her dil bir dosya (`crates/wish_editor/lang/<kod>.json`, çevirmen Rust bilmeden çalışır, `lang/README.md`). 12 dil: İngilizce, Türkçe, Almanca, Fransızca, İspanyolca, Portekizce, İtalyanca, Felemenkçe, Lehçe, Rusça, Ukraynaca, Yunanca. Eksik metin İngilizce görünür; `tests/languages.rs` her dosyanın eksiğini/fazlasını, `{ad}`ların eşleşmesini, yazı tipinde her harfin olduğunu ve menü çubuğunun her dilde 1280 px'e sığdığını denetler. Dil iş parçacığı başına (yan yana koşan testler birbirini bozmaz). Seçim: proje menüsü → Dil; kişinin tercihlerinde saklanır (`WISH_CONFIG_DIR`, `%APPDATA%\Wish\editor.json`, `~/.config/wish/editor.json`); ayrıca `WISH_LANG`, sistemin `LANG`'i, `wen edit --lang` (`unknown_language`). Başsız resim (`--headless-frame`) yalnızca argümanlarına bağlıdır (tercih dosyası okunmaz). Motorun tanı iletileri İngilizce kalır (M10 notu).
- **Cila:** gösterilemeyen sahne (ör. yeni klonlanmış projede derlenmemiş oyun kodunun türleri) görünümde bir kartla nedenini, ipucunu ve "Kodu derle"/"Sorunları göster" düğmelerini gösterir — eskiden görünüm boş, Sorunlar (0) idi (vitrin oyununun kopyasında bulundu). Durum çubuğu tek satır (tamamı üstüne gelince). Kısayollar: Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z, Ctrl+P (yazarken değil). **Hata düzeltildi:** pencere yeniden boyutlanınca başka bir şey olana dek yeniden çizilmiyordu. Dar pencerede menü çubuğu ikinci satıra geçer. Görünüm adları çevrilir (araç adı üstüne gelince).
- **Streamable HTTP (`wish_mcp::http`, M4'ten devir):** `wen mcp --http <port ya da adres>`; tek uç `/mcp`, `POST` (istek → `200` JSON, bildirim/yanıt → `202`), `DELETE` oturumu bitirir, `GET` → `405` (sunucu akışı M10). `initialize` yanıtında `Mcp-Session-Id`; sonra yoksa `400`, bilinmiyorsa/bittiyse `404`. Güvenlik (spesifikasyonun önerileri): varsayılan `127.0.0.1`; döngü dışı adres jetonsuz reddedilir (`http_needs_token`; `WISH_MCP_TOKEN` → `Authorization: Bearer`); `Origin` başka sitedense `403` (DNS rebinding; `WISH_MCP_ORIGINS`), `MCP-Protocol-Version` denetimi, gövde 8 MiB / başlık 64 KiB, 30 sn zaman aşımı, en çok 64 bağlantı, parçalı aktarım `501`. tokio yok: `std::net`, bağlantı başına iş parçacığı; çağrılar çalışma alanının iş parçacığında sırayla; `notifications/cancelled` bağlantı iş parçacığında hemen işler (stdio ile ortak `Busy`). Testler `wish_mcp/tests/http.rs` (4: oturum yaşam döngüsü, gerekçeli retler, başka bağlantıdan iptal, jeton) + birim testleri; curl ile `wen mcp --http` denendi. MCP Inspector'ın HTTP denemesi yapılmadı (Inspector disk için silinmişti; yeniden kurulum indirme ister).
- **Testler:** editörde `settings.rs` (2), `panels.rs` (5), `languages.rs` (4), `polish.rs` (3) + i18n birim testleri; toplam editör testi 70 (release).

### WP9 — eşlik denetimi, "yalnızca elle" çıkış senaryosu, kapanış incelemesi

- **Eşlik tablosu (`wish_editor::parity::PARITY`) ve testi (`tests/parity.rs`):** kataloğun her aracı, `edit_apply`'ın her işlemi (`wish_control::ops::OPS`), `edit_apply` + `session` ve `edit_history`'nin list/undo/redo eylemleri için bir satır: kişinin editörde bunu nerede yaptığı ve bunu elle yapan testin adı (`dosya::işlev`). Test yeni bir araç ya da işlem satırsız eklenirse kırılır; her satırın testi gerçekten var mı diye kaynağa bakar. 30 işlem + 12 soru (sorular — `scene_get`, `describe_view`, `physics_query`... — editörün gösterdiği şeyle karşılanır).
- **Denetimde bulunan açıklar ve kapatılması** (değişmez 9): prefab üzerine yazımını kaldırma (`remove_override` → denetçide "Prefab'a geri döndür"; `entity_get` artık bir parçanın `overridden` bileşenlerini söyler), parçada `replace_override` ("Değerleri yapıştır" artık bileşeni değiştirir, parçada "Varsayılanlara döndür" yok), varlık notları (`set_notes` → denetçinin üstünde "Notlar"), **sahne kaynakları** (`set/replace/remove_resource` → hiçbir şey seçili değilken denetçi sahnenin kaynaklarını gösterir; "Kaynak ekle...", oynarken canlı), yer tutucu dosyalar (`asset_create` → tarayıcıda "Yeni...": zıplama/jeton/gümleme/bip sesi, daire/kare/elmas resmi), geçmiş listesi (`edit_history list` → alt panelde **Geçmiş** sekmesi: kim, ne, kaç değişiklik; Geri al/Yinele), `test_run` (proje menüsünde "Testleri çalıştır" — başarısızlar Sorunlar'da — ve "Altın resimleri güncelle"), `replay` (menü çubuğunda **Kaydet** anahtarı → oynat/durdur kaydı `replays/take_N` olarak saklar; proje menüsünde "Tekrar kayıtları" listesi → geri oynatır ve "oyun aynı oynuyor" ya da ilk sapan tick'i söyler), `capture_frame` ("Resmi kaydet" → `captures/<sahne>_N.png`). Testleri `tests/parity_ops.rs` (8).
- **Çıkış ölçütü 1, uçtan uca (`tests/manual_only.rs`):** vitrin oyununun kopyası yeni klonlanmış gibi açılır (derlenmiş kod yok); görünümdeki karttan **Kodu derle**; yeni bir ada eklenir (+, F2 ile ad, "Bileşen ekle" ile Primitive ve Collider, denetçiden ölçek ve konum), ondan **prefab** yapılır ve tarayıcıdan ikinci bir örnek konup taşınır; **Ctrl+Z / Ctrl+Y**; **Kaydet** + Ctrl+P ile oynanır, durdurulur, kayıt **geri oynatılır** ("aynı oynuyor"); **Testleri çalıştır**: yeni ada sahneyi büyüttüğü için güneşin gölge haritası daha geniş alanı kapsar ve kazanma resminin gölgeleri biraz kayar (1 test kalır, ipucu "resme bak") → kişi **Altın resimleri güncelle** → 4/4 geçer; **Resmi kaydet**; **Denetle** → sorun yok; geçmişteki her kayıt `human`. Testin kendisi hiçbir değiştiren araç çağırmaz (yalnızca okumak için). Release'te ~11 sn.
- **Bulgu (motor davranışı, belgelendi):** tek gölge haritası sahneye oturtulduğu için uzaktaki bir nesne eklemek görünür gölgelerin keskinliğini değiştirir (M3 WP5 kararı). Kademeli gölge haritaları M10'daki gelişmiş render işinde.
- **Bağımsız kapanış incelemesi (3 ajan: editör, kontrol düzlemi, HTTP) — 35 bulgu, hepsi ele alındı.** Regresyon testleri: `wish_control/tests/review_m8a.rs` (5), `wish_editor/tests/review_m8a.rs` (7), `wish_mcp/tests/http.rs` (+5).
  - *Kontrol düzlemi:*
    - Prefab düzenlemesi artık prefab kurallarıyla denetlenir (`prefab_edit_invalid`): kök silinemez ya da yeniden adlandırılamaz; ebeveynsiz varlık, kaynak ve örnek eklenemez.
    - Bir sahnenin üzerine yazdığı prefab varlığı silinemez ya da yeniden adlandırılamaz (`prefab_override_orphaned`; hangi sahne ve örnek olduğunu söyler). Geri al/yinele de aynı denetimden geçer (`commit_document`).
    - Canlı düzenleme `SceneId` ve `GlobalTransform`'u ve verilen `order`'ı reddeder.
    - `project_settings` varsayılan sahneyi diskteki yazımıyla saklar (`path_case_mismatch`; varsayılan sahne de `exact_case`'ten geçer).
    - `project_settings` önce `input.json`'u yazar, `wish.json` yazılamazsa onu geri koyar. Girdi haritasını kendi kanonik biçiminde yazar. Bozuk `input.json`'da `get` projenin adını yine gösterir (`input_problem`).
    - Dışarıda bozulup elle geri alınan sahne artık her düzenlemeyi reddetmez.
  - *Editör:*
    - Dosya adından üretilen kimlikler sahne kuralına uyar: küçük harf, harfle başlar, en çok 56 karakter + `_N`.
    - Oynarken Geri al/Yinele, sahne değiştirme ve "Prefab yap" kapalı (dosyalarla ilgilidirler).
    - Sahne değişince denetçi ve tutamaç önbelleği sıfırlanır; başka sahnenin kaynakları gösterilmez.
    - Notlar yazıldıkları varlığa gider.
    - Tutamaç sürüklemesi, başladığı revizyonu `base_revision` olarak gönderir; arada ajanın yaptığı değişiklik ezilmez.
    - Geri sarınca zaman çizelgesinin terk edilen kısmı silinir; etiketler tekrarlanmaz.
    - Tuş listesine virgül yazılabilir.
    - Başka dosyadaki bir sorun tıklanınca o dosya açılır.
    - Prefab parçasında F2 ve Sil bir şey yapmaz.
    - Başarısız taşımada seçim yerinde kalır.
    - Düzenlenmeyen geçişlerin doku serbest bırakmaları kaybolmaz.
    - Rozetler, derece ve yuva adları çevrilir.
    - Durdur'dan sonra oyunda yaratılan varlık seçili kalmaz.
    - Sonraya kalan: denetçide sayı sürüklemesinin başlangıç revizyonu.
  - *HTTP:*
    - Her oturum kendi istemci adıyla (`actor`) ve protokol sürümüyle çalışır; iki istemci bağlıyken geçmiş artık doğru kişiyi yazar.
    - İptaller (oturum, kimlik) ile anahtarlanır ve bayatlamaz: bitmiş bir çağrının geç gelen iptali, aynı kimlikli sonraki çağrıyı durdurmaz; bir oturum diğerinin çağrısını durduramaz.
    - Süre istek başınadır; yavaş gönderen istemciler 64 yuvayı tutamaz.
    - Başlıklar gövde okunmadan denetlenir; gövde kopyalanmaz.
    - Bekleyen çağrı sınırı 16; iptaller ve bildirimler her zaman geçer, bildirimler hemen `202` alır.
    - `Host` denetimi (DNS rebinding).
    - Jeton sabit zamanlı karşılaştırılır; `Bearer` büyük/küçük harfe duyarsız.
    - Çift `Content-Length` ya da `Host` reddedilir; başlık taraması doğrusal.
    - Bağlantı yuvası panikte de geri verilir.
    - Oturum kimlikleri rastgele; en çok 32 oturum.
    - Toplu ileti `400` alır; `--http` yalnızca `wen mcp` için.
    - **ADR-022'ye uyum:** `wen mcp --http` artık her zaman jeton ister; `WISH_MCP_TOKEN` yoksa rastgele bir jeton üretip başlarken gösterir.
    - Tarayıcıdan doğrudan bağlanma (CORS) yok: M10 notu.

## 9. M8a sonucu (17 Kasım 2026)

**Çıkış ölçütleri:**
1. *Yapay zekâsız, yalnızca elle: vitrin oyununun bir bölümü kurulur, oynanır, dışa aktarıma hazırlanır.* ✅ Kanıt `wish_editor/tests/manual_only.rs` (WP9):
   1. derlenmemiş kopya açılır, Kodu derle;
   2. yeni bir ada, prefab'ı ve ikinci bir örneği eklenir;
   3. Ctrl+Z / Ctrl+Y;
   4. kaydederek oynanır ve kayıt tekrar oynatılır ("aynı oynuyor");
   5. testler çalışır, altın resim güncellenir: 4/4;
   6. resim kaydedilir; Denetle: "sorun yok".

   Geçmişin tamamı `human`. Dışa aktarımın kendisi (`wen export`) M9'da.
2. *Eşlik denetimi (otomatik liste, test).* ✅ `wish_editor::parity::PARITY` + `tests/parity.rs`: kataloğun 26 aracı, 15 düzenleme işlemi, canlı düzenleme ve geçmiş eylemleri. Her satırın onu elle yapan bir testi var; yeni bir araç ya da işlem satırsız eklenemez.
3. *Her eylem bir kontrol düzlemi işlemidir; geri al/yinele her eylemde çalışır.* ✅
   - Her editör eylemi bir `Workspace::call` çağrısıdır (`human`).
   - Doğrudan dosya okuma yalnızca gösterim içindir: tekrar listesi, küçük resimler, boş ad denetimleri, tercih dosyası.
   - Bir sürükleme geçmişte tek kayıttır.
   - Geri al/yinele menüde, kısayolla ve Geçmiş sekmesinde çalışır (oynarken kapalı).
4. *Wasm'da yeni tanımlanan bir bileşen, ek arayüz kodu olmadan denetçide düzenlenir.* ✅ `tests/inspector.rs::a_component_defined_in_game_code_is_edited_without_editor_code` (WP4).

**Sayılar:**
- Editör testleri: 22 dosya, release ve debug.
- Tam kapı temiz: fmt, clippy `-D warnings`, `cargo test --workspace`.
- 12 dilli arayüz.
- MCP: 26 araç; stdio + Streamable HTTP.

**Açık kalanlar (kullanıcı ya da sonraki iş):**
- CI, GitHub faturalandırma sorunu yüzünden çalışmıyor: yalnızca yerelde doğrulandı, Linux'ta M8a doğrulanmadı.
- MCP Inspector ile HTTP denemesi yapılmadı (Inspector yeniden kurulmalı; indirme ister).
- Pencere döngüsünde elle deneme (`wen edit`).
- Denetçide sayı sürüklemesinin başlangıç revizyonu.
- Tarayıcıdan doğrudan HTTP (CORS) ve sunucu akışı (M10).
