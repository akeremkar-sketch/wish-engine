# M9c — Yapay zekâ modu ("Elle ⇄ Yapay zekâyla")

**Durum:** WP1–WP7 kod tarafı bitti, 17 Kasım 2026 (§7). Kullanıcı denemesi bekliyor. Kullanıcı isteğiyle; kapsam onaylı ("burayla gerçekten iyi uğraş").
**Önerilen model · efor:** Opus 5.5 · xhigh.

## 1. Sorun

Bugün editör tek bir yüz gösteriyor: elle düzenlemenin panelleri ve yanında küçük bir Ajan paneli. Yapay zekâyla oyun yapan biri için bu, işin yanlış yerini öne çıkarıyor. Hiyerarşi ve denetçi ekranı kaplıyor; oyunun **ne olduğu**, **ne kararlaştırıldığı** ve **sırada ne olduğu** hiçbir yerde durmuyor.

Ajanlar en çok bağlam eksikliğinden kötü oyun yapar:
- her istek sıfırdan başlar;
- verilen kararlar unutulur;
- kişiye ait seçimler (zorluk, renk, his) ajan tarafından tahminle doldurulur;
- oyunun oynanınca nasıl hissettirdiği ajana hiç ulaşmaz.

## 2. Karar: iki mod, tek proje

Üst çubukta her zaman görünen bir anahtar var: **Elle | Yapay zekâyla**.
- Aynı proje, aynı geçmiş, aynı dosyalar. Mod yalnızca ekranı değiştirir; değişmez 9 korunur.
- **Elle:** bugünkü editör.
- **Yapay zekâyla:** elle düzenleme panelleri gizlenir; yerine oyunu anlatan, ajanı yönlendiren ve sonucu oynayarak değerlendiren bir çalışma alanı açılır.

### Yapay zekâ modunun parçaları ve gerekçeleri

| Parça | Yer | Ne işe yarar (daha iyi oyun için neden) |
|---|---|---|
| **Oyun kartı** | Sol | Oyunun kimliği. Alanlar: ad, tek cümle, tür, oyuncunun amacı, temel döngü ("her 10 saniyede ne yapıyor"), kontroller, görünüm ve his, hedef kitle, **olmazsa olmazlar**, **sonraya kalanlar**. Ajan her görevde bunu okur; kapsam kayması ve tutarsızlık azalır. Kişi doldurur ya da "Ajan taslak çıkarsın" der. |
| **Kararlar** | Sol (sekme) | "Zıplama 2,5 m", "pastel renkler", "şiddet yok" gibi sabitlenmiş kurallar. Kimin koyduğu yazar. Ajan her görevde uyar; ajan bir karar önerince kart olarak gelir, kişi onaylar. Oturumlar arası hafıza budur. |
| **Sohbet ve görev kartları** | Orta | Konuşmanın kendisi. Her görev bir kart: istek, ajanın adımları (araç çağrıları kısa), sonucu, **önce/sonra resmi**, "Geri al" ve "Devam et". Sohbet hafızası korunur (Claude Code oturumu sürdürülür); "Yeni sohbet" sıfırlar. |
| **Sorular** | Ortada, giriş kutusunun üstünde | Kişiye ait seçimlerde ajan tahmin yürütmez, **seçenekli soru** sorar ("Düşman sayısı?" 3 / 5 / 10). Kişi bir seçeneğe dokunur ya da kendi cevabını yazar; isterse cevap karar olarak saklanır; ajan kaldığı yerden sürer. |
| **Hazır istekler** | Giriş kutusunun altında | Sık işler için yönlendirilmiş istekler: seviye ekle, düşman ekle, ses ekle, "oyun hissi" (cilalama), hataları düzelt, oyunu dengele. Her biri oyun kartını ve kararları kullanan iyi kurulmuş bir istek gönderir. |
| **Canlı oyun** | Sağ üst | Oyun görünümü; Oynat, Durdur, Baştan. Ajan çalışırken sahne canlı değişir. |
| **Oyun testi notları** | Sağ, canlı oyunun altında | Oynarken "Not al": o anın tick'i, sahnesi ve resmi notla saklanır ("burası çok zor"). "Ajana gönder" notları tek göreve çevirir. Oyunun hissi ajana böyle ulaşır; yapay zekâyla oyun yapmanın en çok eksik kalan parçası budur. |
| **Plan** | Sağ | Adım listesi; ajan yazar, kişi ekler, sıralar, işaretler. "Sıradakini yap" sıradaki adımı görev olarak başlatır. Büyük işler küçük, denetlenebilir adımlara bölünür. |
| **Sağlık** | Sağ alt | Sorunlar, testler, son derleme. Kırmızıysa "Ajan düzeltsin". Ajanın bozduğunu kişi hemen görür. |
| **Yapay zekâ ayarları** | Üst çubukta düğme | Yardımcı (Claude Code bulundu mu), **model**, izin kipi, sohbet hafızası, her görevden önce denetim noktası, ajanın neye dokunabileceği (sahneler / oyun kodu), bu oturumun harcaması. |

## 3. Kontrol düzlemi (değişmez 4, 8, 9)

**Dosya:** `design/game.json`. Kanonik JSON, git dostu, projenin parçası; varlık değil, belge de değil (`design/` varlık dizinine girmez).

```json
{
  "format": 1,
  "brief": {
    "title": "", "pitch": "", "genre": "", "goal": "", "loop": "", "controls": "",
    "look": "", "audience": "", "must": [], "later": []
  },
  "decisions": [{ "id": 1, "text": "...", "by": "human" }],
  "plan": [{ "id": 1, "text": "...", "status": "todo", "by": "agent:claude-code" }],
  "notes": [{ "id": 1, "text": "...", "scene": "scenes/main.scene.json", "tick": 120, "picture": "design/notes/1.png", "status": "open" }],
  "questions": [{ "id": 1, "text": "...", "options": ["3", "5", "10"], "answer": null }],
  "next_id": 2
}
```

**Araç `game_design` (32.):**
- `get`
- `set_brief` (yalnızca verilen alanlar)
- `decide`, `forget`
- `plan_add`, `plan_set`, `plan_remove`
- `note`, `note_done`
- `ask`, `answer` (`remember: true` → karar)

Değiştiren her eylem `dry_run` alır. Geçmişe ve etkinlik akışına `actor` ile girer; okuma (`get`) akışa girmez. Kimlikler belirlenimci (`next_id`), zaman damgası yok.

**Ajana giden bağlam:** `wish_agent::Task` bir `design` özeti taşır (oyun kartı, kararlar, açık plan adımları, cevaplanan sorular). `instructions` bunu isteğin önüne koyar ve ajana şunları söyler:
- oyun kartını ve planı güncel tut;
- kişiye ait seçimde `game_design ask` ile seçenekli soru sor ve dur;
- kalıcı kuralları `decide` ile önerip kaydet.

**Claude Code bağdaştırıcısı:**
- `model` seçeneği (`--model`);
- sohbet hafızası (`--resume <oturum>`: akıştaki `session_id`);
- harcama (sonuç olayındaki `total_cost_usd`) bir `Event::Usage` olarak gelir.

## 4. Eşlik (değişmez 9)

`game_design`'ın her eylemi yapay zekâ modunun panellerinden elle yapılabilir: kartı yazmak, karar eklemek ve silmek, plan, not, soruya cevap. `wish_editor::parity::PARITY`'e satır ve elle yapan test eklenir.

## 5. İş paketleri

1. `game_design` aracı, dosya, testler, MCP sayısı, eşlik satırları.
2. Ajan bağlamı: tasarım özeti, model, sohbet hafızası, harcama.
3. Mod anahtarı ve yapay zekâ modu yerleşimi.
4. Oyun kartı, kararlar, sorular, plan, sağlık panelleri.
5. Oyun testi notları; hazır istekler; ayarlar penceresi.
6. Sembol ve açılış animasyonu.
7. Çeviriler, kılavuzlar, testler, altın resimler, kapanış incelemesi.

## 6. Test planı

- **`wish_control`:**
  - `game_design` eylemleri;
  - `dry_run`;
  - geçersiz girdi ipuçlu hata;
  - dosya kanonik ve yeniden yüklenince aynı;
  - geçmiş ve etkinlik.
- **`wish_agent`:**
  - talimatlarda oyun kartı ve kararlar;
  - `--model` ve `--resume` argümanları;
  - akıştan `session_id` ve harcama.
- **`wish_editor`:**
  - mod anahtarı yerleşimi değiştirir ve tercihte kalır;
  - kart elle doldurulur → dosya;
  - soru seçeneğine dokunmak → cevap ve karar, sahte ajan devam görevi alır;
  - plan "Sıradakini yap" görev başlatır;
  - oyun testi notu tick ve resimle saklanır, "Ajana gönder" tek görev;
  - eşlik testleri.

## 7. Sonuç (17 Kasım 2026)

- **Kontrol düzlemi:** `game_design` (32. araç, `wish_control/src/tools/design.rs`), `design/game.json` kanonik, `design/` varlık değil (`wish_asset::NON_ASSET_FOLDERS`), yeni tanılar `design_file_invalid`, `design_item_not_found`, `invalid_design`. `wish_control::design_summary` ajanın okuduğu özeti üretir. Testler: `wish_control/tests/game_design.rs` (7).
- **Ajan:** `wish_agent::Task` artık `design`, `model`, `resume`, `files` taşır (`Task::new`); talimatlar `DESIGN_RULES` ile tasarımı ve "kişiye ait seçimde sor ve dur" kuralını verir. Claude Code: `--model`, `--resume` (yalnızca güvenli sözcükler), `files: false` → yalnızca editörün araçları; akıştan `Event::Session` ve `Event::Cost`.
- **Editör:** `wish_editor::ai_mode` (oyun paneli, sohbet + soru kartları + hazır istekler, plan / notlar / sağlık panosu, ayarlar penceresi, canlı görünüm Oyun | Sahne). Mod ve ajan ayarları tercihlerde (`ai_mode`, `agent`). Karşılama ekranında asistan seçilirse editör yapay zekâ modunda açılır. Her görev kartı kendi denetim noktasıyla geri alınır ve önce/sonra karşılaştırılır. Sahne görünümünde tıklamak, seçmek yerine nesneyi isteğe ekler. Testler: `wish_editor/tests/ai_mode.rs` (6; eşlik satırı `ai_mode::the_design_is_made_by_hand`).
- **Sembol:** önce lades kemiği yapıldı; kullanıcı isteğiyle **dilek çubuğu** oldu (`vector::draw_wand`). Krem değnek, turuncu sap halkaları ve uç; arkasında savruluşun bıraktığı dört toz, önünde altın rengi dilek ışığı. Açılışta 2 sn animasyon: değnek aşağıdan savrulur, ucu yay çizer, küçük bir silkinmeyle durur; yay tozlara dağılır, ışık parıltıyla yanar (tıklama ya da tuş atlar; `LaunchOptions::splash`). Denenip bırakılanlar: kıvrım (bastona benzedi), üç nokta ("yükleniyor" gibi), sürekli yay (tırpana benzedi). Pencere ve görev çubuğu simgesi (`wish_editor::window_icon`), kısayol simgesi `wish-engine.ico`, yapay zekâ modunun simgesi `Icon::Wish`. Dört köşeli yıldız/kıvılcım yalnızca parçacıklar için kaldı (Gemini benzerliği).
- **Çeviriler:** 138 yeni metin × 11 dil.
