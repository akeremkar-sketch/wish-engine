# Wish Engine — Kullanıcı kılavuzu

Wish Engine ücretsiz, açık kaynak bir 3B oyun motorudur. İçindeki her şey editörde elle yapılabilir.
Aynı işlerin hepsini MCP üzerinden bir yapay zekâ yardımcısı da yapabilir. İki türlü değişiklik de
aynı geçmişe girer, kimin yaptığı yazılıdır ve her biri tek başına geri alınabilir.

Bu kılavuz bir oyunu ilk açılıştan dışa aktarılmış hâline kadar izler.

- [1. Kurulum ve ilk açılış](#1-kurulum-ve-ilk-açılış)
- [2. Editör](#2-editör)
- [3. Elle düzenleme](#3-elle-düzenleme)
- [4. Oynatma, tekrar kayıtları ve testler](#4-oynatma-tekrar-kayıtları-ve-testler)
- [5. Oyun kodu](#5-oyun-kodu)
- [6. Yapay zekâ yardımcısıyla çalışma](#6-yapay-zekâ-yardımcısıyla-çalışma)
- [7. Oyunu dışa aktarma](#7-oyunu-dışa-aktarma)
- [8. Komut satırı](#8-komut-satırı)
- [9. Bir şey ters gidince](#9-bir-şey-ters-gidince)
- [10. Proje klasöründe neler var](#10-proje-klasöründe-neler-var)

## 1. Kurulum ve ilk açılış

Wish Engine klasörünü istediğiniz yere açın ve `wish-engine.exe`'yi çalıştırın (kısayol için
simge klasördeki `wish-engine.ico`). Başka bir şey kurmanız gerekmez: oyun kodunun derleyicisi
klasörün içinde gelir. Bir proje klasörünü `wish-engine.exe`'nin üzerine bırakırsanız editör o
projeyle açılır. Komut satırında aynı şey `wen edit`'tir.

Proje olmadan editör **karşılama ekranıyla** açılır:

1. **Kim yardım ediyor?**
   - **Claude Code** kuruluysa: editörün Ajan paneli onu kullanır.
   - **Kimse**: oyunu elle yaparsınız.

   Başka bir MCP ajanı sonradan bağlanabilir (proje menüsü, *Dış ajanı bağla...*).
2. **Neyle başlansın?**
   - boş proje (bir kamera, bir güneş, bir zemin ve başlangıç oyun kodu), ya da
   - bir şablon:
     - *Islands*: 3B platform oyunu, vitrin oyunu;
     - *Hop*: 3B zıplama;
     - *Collect*: yukarıdan bakılan 3B;
     - *Bricks*: 2B;
     - *Quiz*: yalnızca arayüz.
3. **Ad ve yer.** Yeni projeler varsayılan olarak `Belgeler/Wish Projects` altına gider.

**Oluştur**'a basın. Motor projeyi yapar, oyun kodunu derler ve editörü aynı pencerede açar.
Sağdaki **Bir oyun aç** son açtığınız projeleri listeler ya da herhangi bir proje klasörünün yolunu
alır.

Dil, karşılama ekranının sağ üstünden, sonra da proje menüsünden (*Dil*) seçilir. 12 dil var.

## 2. Editör

| Yer | Ne gösterir |
|---|---|
| Üst çubuk | Proje menüsü (projenin adını taşır), sahne, *Geri al* / *Yinele*, *Oynat* / *Duraklat* / *Adım* / *Durdur*, görünüm, tutamaç kipi (*Taşı*, *Döndür*, *Ölçekle*, *Dünya* / *Yerel*, *Yapış*), *Ajan* |
| Sol: *Hiyerarşi* | Sahnenin nesneleri, ağaç olarak. *Ara* süzer. |
| Orta | **Sahne** görünümü: sahne, editörün kendi kamerasından. Tıklayınca seçer; tutamacı sürükleyince taşır, döndürür ya da ölçekler. Geniş pencerede yanında **Oyun** görünümü vardır: oyuncunun gördüğü oyun, sahnenin kamerasından ve oyunun arayüzüyle (üst çubukta *Oyun görünümü* açar/kapatır). Oyun koşarken sahne görünümü editör kamerasında kalır; koşan oyunun içinde dolaşabilirsiniz. |
| Sağ: *Denetçi* | Seçili nesnenin bileşenleri ve alanları. Her alanın birimi ve aralığı vardır. |
| Alt sekmeler | *Varlıklar* (dosyalar), *Konsol*, *Sorunlar*, *Performans*, *Geçmiş*, *Etkinlik* |
| Durum satırı | Son eylemin ne yaptığı, hata ve uyarı sayısı |

**Görünümde dolaşmak:**
- Sağ fare düğmesini basılı tutup fareyi oynatınca etrafa bakarsınız.
- Basılıyken **W A S D** ilerletir, **Q / E** alçaltır / yükseltir, **Shift** hızlandırır.
- *Kamerayı sıfırla* sahnenin kamerasına döner.

**Kısayollar:**

| Tuşlar | Eylem |
|---|---|
| W / E / R | Taşı / döndür / ölçekle |
| Ctrl+Z | Geri al |
| Ctrl+Y ya da Ctrl+Shift+Z | Yinele |
| Ctrl+P | Oynat / durdur |
| F2 | Seçili nesneyi yeniden adlandır |
| Delete | Seçili nesneleri sil |
| Escape | Sürüklemeyi iptal et |

**Görünümler** (üst çubuk):
- *Gölgeli*: resmin kendisi.
- *Nesne renkleri*: her nesne kendi renginde.
- *Derinlik*, *Normaller*, *Tel kafes*.
- *Çarpışma şekilleri*: fizik şekilleri.
- *Işıklar*: ışıkların erimi.

## 3. Elle düzenleme

- **Nesneler:**
  - Hiyerarşide *Nesne ekle*, seçili bir nesnede *Alt nesne ekle*.
  - Hiyerarşide sürükleyerek ebeveyn değiştirilir.
  - *Yeniden adlandır* (ya da F2) nesnenin adını değiştirir.
- **Bileşenler:**
  - Denetçide *Bileşen ekle...* (yazarak aranır), *Bileşeni kaldır*.
  - *Değerleri kopyala* / *Değerleri yapıştır* bir bileşeni nesneler arasında taşır.
  - *Varsayılanlara döndür* bir bileşeni sıfırlar.
  - Oyunun kendi bileşenleri de burada görünür, editöre kod yazmadan: alanları oyun kodunun
    şemasından gelir.
- **Sahne kaynakları:** bir nesneye değil sahneye ait ayarlar; yerçekimi, gökyüzü ve ortam ışığı
  gibi. *Sahne kaynakları* altındadır.
- **Varlıklar** (alt sekme; projenin dosyaları):
  - Projenin modelleri, resimleri ve sesleri, türe göre süzülür.
  - Görünüme sürükleyince sahneye yerleşir.
  - *Yeni...* yer tutucu bir ses ya da resim yapar.
  - *Yeniden adlandır / taşı...* dosyayı taşır ve ona yapılan bütün başvuruları günceller.
  - *Dışarıda aç* sisteminizin kullandığı programla açar.
- **Prefab'lar:**
  - *Prefab yap* seçimi yeniden kullanılabilir bir parçaya çevirir.
  - Örnekler *{prefab} örneği* diye görünür; değiştirilmiş alanları işaretlidir.
  - *Prefab'a geri döndür* bir örneğin değişikliklerini geri alır.
  - *Varlıklar*'dan bir prefab açınca sahne gibi düzenlenir.
- **Notlar:** her nesne serbest notlar taşıyabilir (denetçideki *Notlar* kutusu). Ajan da okur.
- **Proje ayarları** (proje menüsü, *Ayarlar...*):
  - ad;
  - varsayılan sahne;
  - **girdi haritası**: `move`, `jump` gibi eylemler ve onları süren tuşlar.
    - *Eylem ekle* / *Eylemi kaldır*.
    - Tuş listeleri `Space, W, Up` biçiminde yazılır.
    - *Girdi haritasını kaydet*.
- **Geçmiş** (alt sekme):
  - Her değişiklik: kim yaptı (siz, bir ajan ya da dışarıda düzenlenen bir dosya) ve ne zaman.
  - *Bunu geri al* tek bir değişikliği, eski de olsa, kendi başına geri alır; yeter ki koyduğu
    değer hâlâ yerinde olsun.
- **Denetim noktaları** (proje menüsü, *Denetim noktaları*):
  - *Denetim noktası kaydet* bütün projeyi olduğu gibi saklar.
  - *Buna geri dön* ona geri döner; dönmek de geçmişte bir değişikliktir, geri alınabilir.
  - *Şimdiyle karşılaştır* denetim noktasını ve şimdiyi yan yana gösterir.

Dosyalar doğruluğun kaynağıdır. Bir sahne dosyasını başka bir programda düzenlemek sorun değildir:
editör fark eder, değişiklik geçmişe *dosya* olarak girer. Bozuk bir dosya bildirilir, üzerine
asla yazılmaz.

## 4. Oynatma, tekrar kayıtları ve testler

- **Oynat** oyunu görünümde saniyede 60 tick çalıştırır.
  - Tuşlar girdi haritasındaki eylemleri sürer.
  - **Duraklat**, **Adım** (tek tick) ve **Durdur**: durdurunca sahne eski hâline döner.
  - Oynarken yaptığınız değişiklikler canlıdır ve saklanmaz.
- **Zaman çizgisi:** oynarken önceki bir tick'e dönebilirsiniz. Oyun belirlenimcidir: aynı sahne ve
  tohum her zaman aynı biçimde oynar.
- **Tekrar kayıtları:**
  - *Kaydet* bir koşuyu, tuşlarıyla birlikte kaydeder.
  - Kaydedilen tekrarlar *Tekrar kayıtları* altında listelenir ve oynatılabilir.
  - Tekrar her tick'i kayıtla karşılaştırır ve farklılaşan ilk tick'i söyler.
- **Testler:**
  - `tests/*.test.json` senaryo testleridir: bir sahneden oyna, tuşlara bas, bir şey bekle.
  - *Testleri çalıştır* hepsini koşar; başarısızlık adımı, tick'i ve okunan değerleri söyler.
  - Resim adımları altın görüntülerle karşılaştırılır; *Altın resimleri güncelle* yenilerini
    kaydeder.
- **Kendi penceresinde:** `wen play` oyunu kendi penceresinde oynatır. Oyun kodunu kaydetmek oyunu
  durdurmadan yeni koda geçirir.

## 5. Oyun kodu

Oyun mantığı `scripts/src/lib.rs` içinde Rust ile yazılır; motor onu WebAssembly'ye derler.

- **Derleme:**
  - Proje menüsünde *Kodu derle*, ya da `wen build`.
  - Hatalar dosya, satır ve ipucuyla gösterilir.
  - Derleme başarısız olursa son çalışan kod çalışmaya devam eder.
- **Düzenleme:**
  - *Kodu düzenle* dosyayı kendi düzenleyicinizde açar (`WISH_EDITOR`'ı ayarlayın, örneğin
    `code`).
  - Oynarken kaydetmek kodu oyunun durumunu kaybetmeden yeniden yükler.
- **Kodun yapabildikleri:** bileşen, kaynak ve olay bildirmek (`#[component]`, `#[resource]`,
  `#[event]`), varlıkları sorgulayan, eylemleri okuyan, komut ve olay gönderen, ışın atan, ses çalan
  ve arayüz metni yazan sistemler (`#[system]`).
- **Güvenlik:** oyun kodu korumalı alanda çalışır. Dosyalarınızı okuyamaz, motoru donduramaz;
  içinde bir çökme olursa olduğu satırla bildirilir.
- **Daha fazlası:** SDK başvurusu `wish://docs/sdk`, çalışan tarifler `wish://docs/recipes`
  adresindedir. İkisi de `wen call docs_search --query <sözcükler>` ile ya da bir ajan tarafından
  okunabilir.

## 6. Yapay zekâ yardımcısıyla çalışma

### Yapay zekâ modu

Üst çubuktaki anahtar editörün iki yüzü arasında geçiş yapar: **Elle | Yapay zekâyla**. Aynı proje, aynı geçmiş, aynı dosyalar; yalnızca ekran değişir. Seçiminiz hatırlanır.

*Yapay zekâyla* modunda elle düzenleme panelleri çekilir, yerine şunlar gelir:

| Yer | Ne işe yarar |
|---|---|
| **Oyun** (sol) — *Oyun kartı* | Oyunun kimliği: ad, tek cümle, tür, oyuncunun hedefi, ana döngü, kontroller, görünüm ve his, kimin için, olmazsa olmazlar, sonraya kalanlar. Ajan her görevden önce okur. *Ajan taslağı yazsın* kartı oyunun şimdiki halinden çıkarır. |
| **Oyun** (sol) — *Kararlar* | Her değişiklikte geçerli kurallar ("zıplama 2,5 m", "pastel renkler"). Ajan bunlara uyar. |
| **Canlı oyun** (orta) | Oyuncunun gördüğü. *Sahne*'ye geçip bir şeye tıklarsanız o şey sonraki isteğe eklenir. |
| *Plan* (orta alt) | Küçük adımlar. *Sıradaki adımı yap* ilk bitmemiş adımı ajana verir. |
| *Oyun testi notları* (orta alt) | Oynarken *Not al*: o anın tick'i ve resmi saklanır. *Notları ajana gönder* hepsini tek görev yapar. |
| *Sağlık* (orta alt) | Sorunlar, kodun derlenip derlenmediği, testler. *Ajan düzeltsin* hepsini tek görev olarak verir. |
| **Sohbet** (sağ) | Her görev bir kart: ne istendi, ajan ne dedi ve hangi araçları çağırdı; *Bu görevi geri al* ve *Önce / sonra*. Altta hazır istekler (*Bölüm ekle*, *Oyun hissi*, *Hata bul*...) kutuyu doldurur. |
| *Ajan soruyor* | Zorluk, görünüm, sayı gibi size ait seçimlerde ajan tahmin etmez, seçenekli soru sorar. Bir seçeneğe tıklayın ya da kendi yanıtınızı yazın; *Yanıtı karar olarak sakla* açıksa karar olur ve ajan kaldığı yerden devam eder. |
| *Yapay zekâ ayarları* | Asistan, model (Opus / Sonnet / Haiku), görevler arasında sohbeti hatırlama, her görevden önce denetim noktası, ajanın oyun kodunu kendisi değiştirip değiştiremeyeceği, bu oturumun harcaması. |

Tasarım projede `design/game.json` dosyasındadır (git'e girer, varlık sayılmaz). Ajan aynı şeyi `game_design` aracıyla yapar.

### Ajan paneli (elle modda)

**Ajan paneli** (üst çubukta *Ajan*):
1. *Ajan ne yapsın?* kutusuna ne istediğinizi yazın ve *Gönder*'e basın.
2. *Görünümde işaret et*, sonra nesnelere tıklayın: ajana neyi kastettiğinizi gösterir.
3. Döküm, ajanın söylediklerini ve çağırdığı her aracı gösterir.

**Her görevden önce bir denetim noktası kaydedilir**, yani her zaman geri dönebilirsiniz.

**İzin kipleri** (proje menüsü, *İzinler*):

| Kip | Ne olur |
|---|---|
| *Yalnızca öner* | Ajan önerir; *Etkinlik* sekmesinde *Kabul et* (ya da *Reddet*) diyene kadar hiçbir şey değişmez |
| *Uygula, ben incelerim* | Ajanın değişiklikleri yapılır; *Etkinlik* sekmesinde gözden geçirirsiniz |
| *Tam özerk* | Ajan serbestçe çalışır; her şey yine geçmiştedir |

**Etkinlik** (alt sekme):
- Herkesin ne yaptığını canlı gösterir.
- *Yalnızca ajanlar* kendi değişikliklerinizi gizler.
- Her kart kendi başına geri alınabilir.

**Dışarıdan bir ajan:**
- *Dış ajanı bağla...* bir adres ve bir jeton gösterir: ikisini de herhangi bir MCP istemcisine
  verin.
- Proje klasöründeki Claude Code'un hiçbir şeye ihtiyacı yoktur: projenin `.mcp.json`'u `wen mcp`'yi
  kendisi başlatır.
- Ajan, editörün kullandığı araçların aynısını kullanır (30 kadar). Hepsi elle de yapılabilir.

Motor hiçbir yapay zekâ olmadan eksiksizdir. Yardımcı, aynı işleri yapmanın daha hızlı bir yoludur.

## 7. Paketler, temalar, çalışma alanları ve komut paleti

**Paketler** tema, çalışma alanı, dil, tarif ve şablon getirir. Paket, içinde `wish-package.json` olan bir klasördür.
- *Proje menüsü → Paketler...* kurulu olanları listeler ve klasöründen paket kurar: *Projeye yükle* (projeyle birlikte taşınır; `packages/` ve `wish.lock` hangi dosyaların olduğunu tam olarak kaydeder) ya da *Benim için yükle* (bu bilgisayarda açtığınız her proje için).
- *Kaldır* paketi çöp kutusuna taşır; hiçbir şey silinmez.
- Projenin kilidi denetlenir: bir paketin dosyaları değişirse pencere bunu söyler; değişiklik sizinse *Kilidi yeniden yaz*.
- **Paket dizini:** *Paket dizininden* bölümünde dizin klasörünü yazın (paketlerden oluşan bir klasör; hatırlanır), birkaç sözcük yazıp *Ara*'ya basın. Her paket, bu motorla çalışan en yeni sürümünü gösterir: *Projeye ekle*, *Benim için ekle* ya da daha yeni uyumlu bir sürüm varsa *Güncelle*. Hub'ın *Paketler* sekmesinde kendi paketleriniz için aynı arama vardır.
- Komut satırında: `wen pkg search dark theme --index <klasör>`, `wen pkg add <id>`, `wen pkg update`, `wen pkg remove <id>`, `wen pkg lock` (kendi paketleriniz için `--user`; `WISH_PACKAGE_INDEX` her seferinde `--index` yazmaktan kurtarır).

**Motor sürümleri:** motorun birden çok sürümü yan yana durabilir. Hub'ın *Motor sürümleri* sekmesi onları listeler; *Motor ekle* başka bir motorun klasörünü olduğu yerde hatırlar (`wen` ve `wish-engine.json` içeren klasör; hiçbir şey kopyalanmaz), *Kaldır* birini unutur. Bir proje, yapıldığı sürümle açılır (`wish.json` hangisi olduğunu söyler); o sürüm bilgisayarda yoksa en yeni uyumlu sürümle; o da yoksa bu motor açar ve bunu söyler. Son projeler listesi, başka bir sürümle yapılmış projeyi gösterir. Komut satırında: `wen engines`, `wen engines add <klasör>`, `wen engines pick <proje>`.

**Temalar:** *Proje menüsü → Tema* editörün görünümünü hemen değiştirir: Atelier (varsayılan), Midnight, High contrast ve paketlerin getirdiği her tema.

**Çalışma alanları:** yapay zekâ modunda oyun kartının üstündeki *Çalışma alanı* kutusu kartın ne için olduğunu seçer. *Hikâye oyunu* mekânı, karakterleri, tonu, seçimleri ve sonları sorar ve kendi hazır isteklerini getirir; *Varsayılan* motorun her oyun için kartıdır. Geçiş yaptığınızda yazdığınız hiçbir şey kaybolmaz.

**Komut paleti:** Ctrl+K ile açılır. Ne istediğinizin birkaç harfini yazın (*dışa aktar*, *yapay zekâ*, *test*), ok tuşlarıyla seçin, Enter çalıştırır. Kısayollar: Ctrl+P oynat/durdur, Ctrl+Z/Ctrl+Y geri al/yinele, Ctrl+M elle/yapay zekâ modu, Ctrl+B kodu derle. Birini değiştirmek için tercih dosyanıza ekleyin: `"keys": { "mode.switch": "Ctrl+J" }`.

## 8. Oyunu dışa aktarma

Proje menüsü, *Oyunu dışa aktar* (ya da `wen export`):
- Önce proje denetlenir.
- Oyun kodu derlenmiş olmalıdır.
- Sonuç `exports/<ad>/` klasörüdür:
  - `<ad>.exe`: oynatıcı;
  - `game/`: sahneler, varlıklar ve derlenmiş kod;
  - `LICENSES.txt`.

Oyunu paylaşmak için bu klasörü sıkıştırın. Motoru, editörü ya da derleyicisi olmayan bir
makinede çalışır ve editörde oynadığı gibi oynar.

## 9. Komut satırı

`wen` editörün yaptığı her şeyi yapar. Birkaç kullanışlı komut:

```bash
wen new oyunum --template islands
```

```bash
wen edit --root oyunum
```

```bash
wen play --root oyunum
```

```bash
wen check --root oyunum
```

```bash
wen test --root oyunum
```

```bash
wen export --root oyunum
```

`wen call <araç>` herhangi bir kontrol düzlemi aracını çalıştırır ve JSON cevabını, bir MCP
istemcisinin aldığı biçimde yazar. Örneğin `wen call project_info --root oyunum`. Bilinmeyen bir
araç adı hepsini listeler. `wen help` bütün komutları listeler.

## 10. Bir şey ters gidince

Motorun bildirdiği her sorunda şunlar vardır:
- bir **kod** (`unknown_component` gibi);
- bir ileti;
- yeri: dosya, satır ve alan;
- bir **ipucu**: ne yapılacağı.

Motorun kendisi beklenmedik bir hatayla durursa bilgisayarınıza bir çökme raporu yazar (editör
ayarlarının yanındaki `crashes` klasörüne; dışa aktarılan bir oyun kendi yanına) ve editör bir
sonraki açılışta yerini söyler. Hiçbir şey gönderilmez: düzeltilmesine yardım etmek isterseniz
dosyayı bir hata kaydına ekleyin.

*Sorunlar* sekmesi bütün projedekileri listeler. `wen call docs_search --query <kod>` bir kodu
açıklar.

| Görülen | Genellikle |
|---|---|
| *No component named 'X' is registered* | X'i tanımlayan oyun kodu derlenmemiş: *Kodu derle* |
| *oyun kodu derlenmedi* | Derleyici nedenini aynı satırda söyler; gösterdiği satırı düzeltin |
| *Claude Code bulunamadı* | Claude Code'u kurun ve bir kez oturum açın, ya da `WISH_CLAUDE`'u yoluna ayarlayın |
| *dışa aktarılmadı: ... hata* | *Sorunlar*'ın listelediklerini düzeltin, sonra yeniden dışa aktarın |
| Görünüm siyah | *Sorunlar*'a bakın: sahnede kamera olmayabilir |

Motorun tanı iletileri İngilizcedir (ajanlar ve belgeler için tek dil); editörün arayüzü sizin
dilinizdedir.

## 11. Proje klasöründe neler var

```text
wish.json             proje: ad, varsayılan sahne
scenes/               sahneler (*.scene.json) ve prefab'lar (*.prefab.json)
assets/               modeller (.glb/.gltf), resimler (.png), sesler (.wav/.ogg)
scripts/src/lib.rs    oyun kodu
input.json            eylemler ve tuşları
tests/                senaryo testleri (*.test.json) ve altın resimler
replays/              kaydedilmiş tekrarlar
exports/              dışa aktarılan oyunlar (projenin parçası değil)
.wish/                motorun ürettikleri (derlemeler, denetim noktaları); paylaşılmaz
.mcp.json, CLAUDE.md  bir yapay zekâ ajanının nasıl bağlanıp çalışacağı
```

Her dosya kanonik JSON'dur; git sürümler arasındaki farkı açıkça gösterir.
