# M8b — Yapay zekâ kokpiti

**Durum:** başladı (17 Kasım 2026), dal `m8b-cockpit`. Önerilen model · efor: Opus 5.5 · high (bağdaştırıcı ve seçmeli geri al mimari kararları); arayüz işleri Sonnet 5.5 · high.

## 1. Hedef ve sınır

Kokpit, editörün içine eklenmiş bir sohbet penceresi **değildir**. Yapay zekâyı yönetme ögeleri baştan tasarımın parçasıdır (VISION, ARCHITECTURE "Yapay zekâ kokpiti ögeleri"). Her öge çekirdekte zaten var olan bir mekanizmanın görsel yüzüdür:
- etkinlik akışı ve kart başına geri al,
- denetim noktaları,
- önce/sonra karşılaştırma,
- yetki kipleri,
- göster ve söyle,
- canlı gözlem,
- kokpit içi ajan.

**Çıkış ölçütleri (ROADMAP):**
1. Motoru hiç görmemiş bir kullanıcı, yalnızca konuşarak 30 dakikada oynanabilir bir mini oyun çıkarır (en az 3 kişiyle). **Kullanıcı testi gerekir.**
2. Ajanın yaptığı her değişiklik etkinlik akışında görünür ve tek tıkla geri alınır.
3. Elden ele geçiş: kişi elle değiştirir, ajan görür ve üstüne devam eder; ajan çalışırken kişi başka bir varlığı elle düzenler ve hiçbir değişiklik kaybolmaz.
4. Elle ve ajanla yapılan değişiklikler aynı geçmişte, kimin yaptığı bilgisiyle görünür.
5. Dış ajan (Claude Code) ve kokpit içi ajan aynı anda çalışırken çakışma veri kaybına yol açmaz.

**Kullanıcıya ait olanlar (gece yapılmadı):**
- T1 kokpit maketleri ve kullanıcı sınaması;
- 3 kişilik kullanıcı testi;
- gerçek modelle koşum (kota harcar).

Bu not ve ilk iş paketleri bunları beklemeden yapılabilenleri kapsar. Ajan, testlerde **sahte modelle** (betikli yanıtlar) sınanır.

## 2. Kararlar

### 2.1 Kart başına geri al = seçmeli geri al

Doğrusal geri al yığını kart başına geri almaya yetmez: kartı geri almak, ondan sonra yapılan her şeyi geri almak demek olmamalı.

- Her işlemin tersi geçmişte zaten duruyor (`wish_scene::edit`: her işlem çalışırken kendi tersini hesaplar).
- **Seçmeli geri al:** yalnızca o işlemin tersi uygulanır, ancak sonraki işlemler aynı yere dokunmadıysa. "Aynı yer", JSON göstericilerinin önek örtüşmesidir: `/entities/x` ile `/entities/x/components/T` örtüşür.
- Dokunan bir sonraki değişiklik varsa geri alma **reddedilir**. Tanı hangi revizyonların, kimin tarafından dokunduğunu söyler. İpucu: "önce onları geri al" ya da "elle düzelt".
- Geri almanın kendisi yeni bir işlemdir, geçmişe girer ve geri alınabilir. Aynı kart iki kez geri alınamaz.
- Dışarıdan dosya değişikliği (`replace_content`) eski tersleri geçersiz kılar: o andan önceki kartlar "artık geri alınamaz" diye işaretlenir.

**Neden:** "Kart başına tek tıkla geri al" ile "hiçbir değişiklik kaybolmaz" (çıkış ölçütü 3) ancak böyle birlikte tutar. Örtüşme denetimi, ajanın işini geri alırken kişinin sonradan yaptığı işi sessizce bozmayı önler.

### 2.2 Etkinlik akışı

İki kaynaktan beslenir:

1. **Belge geçmişi** (`edit_history list`): her düzenleme bir kart.
   - Kartın taşıdıkları: kim, ne (işlem özeti, dokunduğu varlıklar), **neden** (`edit_apply` yeni isteğe bağlı `reason` parametresi), geri alınabilir mi.
2. **Çalışma alanı etkinlik günlüğü** (yeni):
   - Değiştiren her araç çağrısı (her aktör) ve ajanların okuyan çağrıları kaydedilir: kim, araç, kısa özet, sonuç.
   - Okuyan çağrılar örneğin resim çekme, test, simülasyon; bunlar "canlı gözlem" içindir.
   - Tool: `activity_get`, `since` ile (eşlik tablosunda karşılığı: Etkinlik paneli).
   - Belirlenimcilik etkilenmez: günlük simülasyonda değil, araç katmanındadır, duvar saati tutmaz (sıra numarası).

### 2.3 Denetim noktaları ve önce/sonra

- `checkpoint` aracı:
  - **save:** projenin sahne ve prefab belgelerinin kanonik metni ile `wish.json` ve `input.json`, `.wish/checkpoints/<ad>/` altına.
  - **list.**
  - **restore:** her belge bir `replace_content` işlemi olarak geri yazılır. Böylece geçmişe girer ve geri alınabilir; kaybolan değişiklik olmaz.
- Kokpit her ajan görevinden önce kendiliğinden bir denetim noktası alır.
- **Önce/sonra:** `capture_frame` + `checkpoint` (bir denetim noktasındaki belgeden resim). Editör iki resmi yan yana gösterir.

### 2.4 Yetki kipleri

Çalışma alanında ajan aktörleri için politika (kişi her zaman tam yetkili):
- **Yalnızca öner:** değiştiren ajan çağrıları `dry_run` olarak çalışır ve bir **öneri** olarak bekletilir; ajan "önerildi, onay bekliyor" yanıtı alır. Kişi kabul edince gerçek çağrı ajanın adıyla uygulanır, reddedince düşer.
- **Uygula, ben incelerim:** bugünkü davranış; kartlar akışta.
- **Tam özerk:** aynı uygulama; ek olarak kokpit, ajan görevleri arasında onay sormaz.

Politika bir kontrol düzlemi ayarıdır (değişmez 4). Dış ajanlar da ona tabidir.

### 2.5 Kokpit içi ajan (ADR-024'ün geçici kısmının kararı)

- **Bağdaştırıcı arayüzü:** `wish_agent` crate'i, sağlayıcıdan bağımsız. Bir görev başlatılır; bağdaştırıcı olaylar akıtır (metin, araç çağrısı, araç sonucu, bitti/hata).
- **İlk bağdaştırıcı: Claude Code alt süreç olarak** (`claude -p --output-format stream-json`), kullanıcının kendi oturumuyla. API anahtarı yok, motor komisyon almaz (ADR-024).
  - Ajan, editörün kendi çalışma alanına bağlanır; bunun için editör kendi MCP uç noktasını (Streamable HTTP, rastgele jeton, yerel adres) açar.
  - Böylece ajanın çağrıları editörün çalışma alanında, aynı geçmişte ve aynı akışta çalışır.
- **Sahte bağdaştırıcı:** testler için. Betikli araç çağrılarını doğrudan çalışma alanına yapar.
- **Neden alt süreç (doğrudan API değil):**
  - Ağ/TLS bağımlılığı eklemez.
  - Kullanıcının mevcut aboneliğini kullanır.
  - Araçlar zaten MCP'de.
  - Claude Code'un kendi ajan döngüsü (bağlam yönetimi, yeniden deneme) bedavaya gelir.

  Doğrudan API bağdaştırıcısı aynı arayüzle sonradan eklenebilir.

### 2.6 Editörün kendi MCP uç noktası

- `wish_mcp::http`'nin çalışma alanını kendi iş parçacığında çalıştıran döngüsü, sahibinin döngüsüne gömülebilen bir **uç noktaya** ayrılır.
- Editör her karede bekleyen işleri kendi çalışma alanında çalıştırır ve yanıtlar.
- Tek çalışma alanı = tek geçmiş, anında görünürlük. Dış ajan da aynı uca bağlanabilir.

## 3. İş paketleri

| WP | İş | Model · efor |
|---|---|---|
| WP1 | Seçmeli geri al (`wish_scene`), `edit_apply reason`, `edit_history revert`, kartların geri alınabilirliği | Opus 5.5 · high |
| WP2 | Etkinlik günlüğü + `activity_get`, editörde Etkinlik paneli (kartlar, "Bunu geri al") | Sonnet 5.5 · high |
| WP3 | Denetim noktaları + önce/sonra | Sonnet 5.5 · high |
| WP4 | Yetki kipleri (öneriler, onay kapısı) | Opus 5.5 · high |
| WP5 | Editörün MCP uç noktası; `wish_agent` (arayüz, sahte, Claude Code alt süreç); konuşma paneli; göster ve söyle bağlamı | Opus 5.5 · high |
| WP6 | Kullanıcı testleri (3 kişi), gerçek model koşumu, kapanış | Kullanıcı + Opus 5.5 · high |

## 4. Test planı

- Seçmeli geri al:
  - örtüşmeyen sonraki değişiklik korunur;
  - örtüşen değişiklik reddedilir, kim ve hangi revizyon söylenir;
  - geri al geri alınabilir;
  - aynı kart iki kez geri alınamaz;
  - dışarıdan değişiklik eski kartları kapatır.
- Elden ele geçiş (çıkış ölçütü 3): sahte ajan bir varlığı değiştirirken kişi başka birini değiştirir. Sonra ajanın kartı geri alınır: kişinin değişikliği yerinde kalır.
- Akış: her kart kim/ne/neden taşır; dış ajan (MCP) ve kokpit ajanı aynı akışta.
