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
