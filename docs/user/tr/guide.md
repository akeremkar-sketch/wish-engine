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
- [7. Paketler, temalar, çalışma alanları ve komut paleti](#7-paketler-temalar-çalışma-alanları-ve-komut-paleti)
- [8. Oyunu dışa aktarma](#8-oyunu-dışa-aktarma)
- [9. Komut satırı](#9-komut-satırı)
- [10. Bir şey ters gidince](#10-bir-şey-ters-gidince)
- [11. Proje klasöründe neler var](#11-proje-klasöründe-neler-var)
- [12. Bu kılavuzdaki iddialar nasıl doğrulanır](#12-bu-kılavuzdaki-iddialar-nasıl-doğrulanır)

## 1. Kurulum ve ilk açılış

Sisteminize uygun arşivi deponun
[Sürümler (Releases) sayfasından](https://github.com/akeremkar-sketch/wish-engine/releases) indirin
(ilki 0.9.0 betasıdır) ya da motoru [README](../../../README.md#getting-started)'de anlatıldığı gibi
kaynak koddan derleyin.

Wish Engine arşivini istediğiniz yere açın; Windows'ta `wish-engine.exe`'yi (kısayol için simge
klasördeki `wish-engine.ico`), Linux'ta `wish-engine`'i çalıştırın. Oyun kodunun derleyicisi
klasörün içinde gelir; hiçbir geliştirici aracı kurmanız gerekmez. Bilgisayarın kendisinde olması
gereken iki şey:
- **Windows:** oyun kodunu derlemek şimdilik Visual C++ çalışma zamanını (`VCRUNTIME140.dll`)
  ister; çoğu Windows bilgisayarında vardır. Motorun, editörün ve dışa aktarılan oyunların buna
  ihtiyacı yoktur.
- **Linux:** glibc 2.35 ya da daha yenisi (Ubuntu 22.04, Debian 12), ALSA kitaplığı
  (`libasound2`), bir Vulkan sürücüsü ya da OpenGL, ve bir masaüstünde bulunan X11 ya da Wayland
  kitaplıkları.

Bir proje klasörünü `wish-engine.exe`'nin üzerine bırakırsanız editör o projeyle açılır. Komut
satırında aynı şey `wen edit`'tir.

Proje olmadan editör **karşılama ekranıyla** açılır:

1. **Kim yardım ediyor?**
   - **Claude Code** kuruluysa: editörün Ajan paneli onu kullanır.
   - **Kimse**: oyunu elle yaparsınız.

   Başka bir MCP ajanı sonradan bağlanabilir (*Yapay zekâ* menüsü, *Dış ajanı bağla...*).
2. **Nasıl bir oyun?** **3B** (uzayda modeller, ışıklar ve bir kamera) ya da **2B** (düz ekranda
   sprite'lar). Tür yalnızca başlangıç noktasını belirler: 2B proje ortografik bir kamerayla ve
   zemin olmadan başlar, *Oluştur* menüsü önce sprite'ları sıralar; 2B bir proje yine de 3B şeyler
   içerebilir, tersi de geçerlidir. Tür `wish.json` içine yazılır (`"kind": "2d"`); türü olmayan
   proje 3B projedir.
3. **Neyle başlansın?** O türün şablonları:
   - 3B: boş proje (bir kamera, bir güneş, bir zemin ve başlangıç oyun kodu) ya da bir şablon:
     *Islands* (3B platform oyunu, vitrin oyunu), *Hop* (3B zıplama), *Collect* (yukarıdan bakılan
     3B), *Quiz* (yalnızca arayüz);
   - 2B: boş proje (ortografik bir kamera ve başlangıç oyun kodu) ya da *Bricks*.
4. **Ad ve yer.** Yeni projeler varsayılan olarak `Belgeler/Wish Projects` altına gider.

**Oluştur**'a basın. Motor projeyi yapar, oyun kodunu derler ve editörü aynı pencerede açar.
Sağdaki **Bir oyun aç** son açtığınız projeleri listeler ya da herhangi bir proje klasörünün yolunu
alır.

Dil, karşılama ekranının sağ üstünden, sonra da *Görünüm* menüsünden (*Dil*) seçilir. 12 dil var.

## 2. Editör

| Yer | Ne gösterir |
|---|---|
| Menü çubuğu | *Dosya*, *Düzen*, *Oluştur*, *Görünüm*, *Oyun*, *Yapay zekâ*, *Yardım*; proje ve sahne; *Her şeyde ara* (Ctrl+K, komut paleti) |
| Araç çubuğu | *Geri al* / *Yinele*, sahne, *Oynat* / *Duraklat* / *Adım* / *Durdur*, *Kaydet*, *Elle* / *Yapay zekâyla*, *Ajan* |
| Sol: *Hiyerarşi* | Sahnenin nesneleri, ağaç olarak. *Nesneleri süz* daraltır; alt öğesi olan satır kaç tane olduğunu gösterir. |
| Orta | **Sahne** görünümü: sahne, editörün kendi kamerasından. Tıklayınca seçer; tutamacı sürükleyince taşır, döndürür ya da ölçekler. Geniş pencerede yanında **Oyun** görünümü vardır: oyuncunun gördüğü oyun, sahnenin kamerasından ve oyunun arayüzüyle (görünümün sağ üstündeki *Oyun görünümü* açar; oyun görünümünün kendi köşesi kapatır). İki görünüm arasındaki çizgiyi sürükleyerek ortayı paylaştırırsınız (çift tık ortalar); köşedeki düğme, *Görünüm > Görünümü büyüt* ya da görünümün üzerindeyken **Shift+Boşluk** onu editörü dolduracak kadar büyütür, aynısı paneller geri getirir. Oyun koşarken sahne görünümü editör kamerasında kalır; koşan oyunun içinde dolaşabilirsiniz. Tutamaç araçları (*Taşı*, *Döndür*, *Ölçekle*, *Dünya* / *Yerel*, *Yapış*, *Kamerayı sıfırla*) görünümün sol üstünde durur. |
| Sağ: *Denetçi* | Seçili nesnenin bileşenleri ve alanları. Her alanın birimi ve aralığı vardır. |
| Alt sekmeler | *Varlıklar* (dosyalar), *Konsol*, *Sorunlar*, *Performans*, *Geçmiş*, *Etkinlik* |
| Durum satırı | Son eylemin ne yaptığı, hata ve uyarı sayısı, kaç nesnenin seçili olduğu |

**Görünümde dolaşmak:**
- Sağ fare düğmesini basılı tutup fareyi oynatınca etrafa bakarsınız.
- Basılıyken **W A S D** ilerletir, **Q / E** alçaltır / yükseltir, **Shift** hızlandırır.
- **Alt** ve sol tuş öndeki noktanın çevresinde döndürür (yörünge); orta tuş kaydırır; tekerlek
  yakınlaştırır; **F** seçileni çerçeveler (*Görünüm → Seçileni çerçevele*).
- *Kamerayı sıfırla* sahnenin kamerasına döner.

**Kısayollar:**

| Tuşlar | Eylem |
|---|---|
| W / E / R | Taşı / döndür / ölçekle |
| Ctrl+Z | Geri al |
| Ctrl+Y ya da Ctrl+Shift+Z | Yinele |
| Ctrl+P | Oynat / durdur |
| Ctrl+D | Seçileni (alt öğeleriyle) çoğalt |
| Ctrl+N | Yeni sahne (kamera, zemin ve güneş) |
| Ctrl+Shift+N | Boş bir nesne oluştur (*Oluştur* menüsünde diğerleri var) |
| F | Seçileni çerçevele |
| Ctrl+K | Komut paleti |
| F2 | Seçili nesneyi yeniden adlandır |
| Delete | Seçili nesneleri sil |
| Escape | Sürüklemeyi iptal et |

**Görünümler** (görünümün sağ üstündeki kutu, ya da *Görünüm → Gölgelendirme*):
- *Gölgeli*: resmin kendisi.
- *Nesne renkleri*: her nesne kendi renginde.
- *Derinlik*, *Normaller*, *Tel kafes*.
- *Çarpışma şekilleri*: fizik şekilleri.
- *Işıklar*: ışıkların erimi.
- *Gölge kademeleri*: her yüzeyin hangi gölge haritasını kullandığı (gri: gölge yok).

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
- **Sahnenin görünümü:**
  - Yeni bir sahne motorun varsayılan görünümüyle gelir, *Doğal*: sahneyi gökyüzü aydınlatır,
    küçük bir güneş diski, uzakta hafif bir pus, parlak ışıkta yumuşak bir parıltı, hafif renk
    ayarı, yumuşatılmış kenarlar ve kameranın yakınında keskin gölgeler.
  - *Sahne kaynakları*, *Kaynak ekle...*, *Görünüm uygula...* bütün bir görünümü tek adımda
    ayarlar: *Sade* (hiç efekt yok), *Doğal* ya da *Canlı* (Doğal'ın daha güçlüsü). Değerleri
    sahneye yazar; tek bir geri alma hepsini geri alır.
  - Her parçası sonradan elle değiştirilebilir: *Sahne kaynakları* altına `Fog`, `Bloom`,
    `ColorGrading`, `Sky`, `Ambient` ya da `RenderSettings` ekleyip alanlarını düzenleyin.
  - Oyun içi arayüz, sprite'lar ve parçacıklar biten resmin üstüne çizilir: görünüm onları
    değiştirmez. *Orthographic* (2B) ayarlı bir kamera güneş diskini hiç göstermez.
  - Motorun eski bir sürümüyle yapılmış sahne, eski görünümünü korur (bkz. bölüm 10).
- **Varlıklar** (alt sekme; projenin dosyaları):
  - Projenin modelleri, resimleri ve sesleri, türe göre süzülür.
  - Görünüme sürükleyince sahneye yerleşir.
  - *Yeni...* yer tutucu bir ses ya da resim, bir *Malzeme dosyası* ya da seçili resmin
    *Sprite sayfası*nı yapar.
  - *Yeniden adlandır / taşı...* dosyayı taşır ve ona yapılan bütün başvuruları günceller.
  - *Dışarıda aç* sisteminizin kullandığı programla açar.
- **Görünüşler (malzemeler ve dokular):**
  - Bir şeklin görünüşü, denetçideki `Material` bileşenidir: düz bir görünüş için renk, metal,
    pürüzlülük ve ışıma.
  - Yüzeydeki resim bir *Malzeme dosyası*'ndan gelir. Bir tane yapın (*Varlıklar*, *Yeni...*,
    *Malzeme dosyası*), seçin ve alanlarını tarayıcının dosyayı gösterdiği yerde ayarlayın:
    `base_color_texture` projenin bir PNG dosyasını alır (yolunu yazın, kutunun yanındaki
    listeden seçin ya da dosyayı üstüne sürükleyin), `uv_scale` resmi yineler, `alpha_mode`
    *Mask* resmin saydam olduğu yerlerde delik açar, *Blend* yüzeyi alfasına göre saydam yapar
    (cam, su), `double_sided` yüzeyin arkasını da gösterir.
  - Düz bir `Material` da `base_color` alfası (son kutu, `w`, ya da renk seçici) 1'in altındaysa
    saydamdır: 0,5 yarı saydam, 0 görünmez. Saydam yüzeyler gölge düşürmez.
  - Sonra dosyayı bir nesnenin `Material` bileşenindeki `file` alanına yazın. Dosyayı kullanan
    her nesne aynı görünüşü paylaşır ve onunla birlikte değişir; böyle bir nesnenin kendi
    `Material` bileşenindeki renk ve sayılar artık kullanılmaz.
  - Bir modelde malzeme dosyası, modelin kendi malzemelerinin hepsinin yerine geçer. `file`
    yoksa model kendi malzemelerini korur.
  - Bir nesneye dosya yazmak geçmişe girer ve geri alınabilir. Malzeme dosyasının kendisindeki
    değişiklik, kimin yaptığıyla birlikte kaydedilir (*Etkinlik*), ama henüz kendi geri alması
    yoktur: büyük bir değişiklikten önce bir denetim noktası kaydedin.
  - Dokular, kenarı en çok 4096 piksel olan PNG dosyalarıdır. Eksik ya da bozuk bir doku
    bildirilir ve yüzey onsuz çizilir.
- **Sprite'lar ve sprite sayfaları (2B):**
  - Bir `Sprite` sahnede düz bir resim gösterir. `color` onu boyar (tek beyaz karakter, birçok
    renk); `size` onu başka bir boyutta çizer, `slice` kenarlarıyla da dokuz parçalı olur: köşeler
    boyutunu korur, gerisi uzar (paneller ve çerçeveler).
  - Yürüme döngüsü bir *Sprite sayfası*ndan gelir: bir resim seçin, sonra *Varlıklar*, *Yeni...*,
    *Sprite sayfası*. Tarayıcının sayfayı gösterdiği yerde resmin nasıl kesileceğini ayarlayın
    (`grid`: bir karenin `frame` boyutu, `columns`, `rows`; ya da *Dikdörtgenlere geç*), bir ad
    yazıp *Klip ekle*'ye basın ve her klibe `frames` (`[0, 1, 2, 3]`, soldan sağa, sonra aşağı
    numaralanır), `fps` ve `loop` verin.
  - Sonra `Sprite` olan nesneye bir `SpriteAnimator` ekleyin; `sheet` alanına sayfayı, `clip`
    alanına bir klibi yazın. Oyun çalışırken klip oynar; oyun kodu klibi
    `animator.play(text("walk"))` ile değiştirir.
- **Prefab'lar:**
  - *Prefab yap* seçimi yeniden kullanılabilir bir parçaya çevirir.
  - Örnekler *{prefab} örneği* diye görünür; değiştirilmiş alanları işaretlidir.
  - *Prefab'a geri döndür* bir örneğin değişikliklerini geri alır.
  - *Varlıklar*'dan bir prefab açınca sahne gibi düzenlenir.
- **Notlar:** her nesne serbest notlar taşıyabilir (denetçideki *Notlar* kutusu). Ajan da okur.
- **Proje ayarları** (*Dosya → Ayarlar...*):
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
- **Denetim noktaları** (*Düzen → Denetim noktaları*):
  - *Denetim noktası kaydet* projenin sahnelerini, prefablarını, malzeme dosyalarını, meta
    dosyalarını ve ayarlarını olduğu gibi saklar (resimleri ve sesleri saklamaz).
  - *Buna geri dön* ona geri döner; dönmek de geçmişte bir değişikliktir, geri alınabilir.
    Üzerine yazdığı malzeme, meta ve ayar dosyaları `.wish/trash/checkpoints/` içinde saklanır.
  - *Şimdiyle karşılaştır* denetim noktasını ve şimdiyi yan yana gösterir (bugünkü dokularla).

Dosyalar doğruluğun kaynağıdır. Bir sahne dosyasını başka bir programda düzenlemek sorun değildir:
editör fark eder, değişiklik geçmişe *dosya* olarak girer. Bozuk bir dosya bildirilir, üzerine
asla yazılmaz.

## 4. Oynatma, tekrar kayıtları ve testler

- **Oynat** oyunu görünümde saniyede 60 tick çalıştırır.
  - Tuşlar girdi haritasındaki eylemleri sürer.
  - **Duraklat**, **Adım** (tek tick) ve **Durdur**: durdurunca sahne eski hâline döner.
  - Oynarken yaptığınız değişiklikler canlıdır ve saklanmaz.
- **Zaman çizgisi:** oynarken önceki bir tick'e dönebilirsiniz. Oyun belirlenimcidir: aynı sahne,
  tohum ve tuşlar, motorun aynı sürümünde her zaman aynı biçimde oynar.
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
  - *Oyun → Kodu derle* (Ctrl+B), ya da `wen build`.
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

Araç çubuğundaki anahtar editörün iki yüzü arasında geçiş yapar: **Elle | Yapay zekâyla**. Aynı proje, aynı geçmiş, aynı dosyalar; yalnızca ekran değişir. Seçiminiz hatırlanır.

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

**Ajan paneli** (araç çubuğunda *Ajan*, ya da *Yapay zekâ → Ajan panelini göster ya da gizle*):
1. *Ajan ne yapsın?* kutusuna ne istediğinizi yazın ve *Gönder*'e basın.
2. *Görünümde işaret et*, sonra nesnelere tıklayın: ajana neyi kastettiğinizi gösterir.
3. Döküm, ajanın söylediklerini ve çağırdığı her aracı gösterir.

**Her görevden önce bir denetim noktası kaydedilir**, yani her zaman geri dönebilirsiniz.

**İzin kipleri** (*Yapay zekâ → İzinler*):

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
- Ajan, editörün kullandığı araçların aynısını kullanır (34 araç). Hepsi elle de yapılabilir.

Motor hiçbir yapay zekâ olmadan eksiksizdir. Yardımcı, aynı işleri yapmanın daha hızlı bir yoludur.

## 7. Paketler, temalar, çalışma alanları ve komut paleti

**Paketler** tema, çalışma alanı, dil, tarif ve şablon getirir. Paket, içinde `wish-package.json` olan bir klasördür.
- Paket, çalıştığı motor sürümlerini `engine` alanında söyler. Motorun sürümü 0 ile başladığı sürece `">=0.2"` gibi bir alt sınır yazın: tek başına `"0.2"` yalnızca 0.2.x demektir ve daha yeni her 0.x motoru paketi reddeder (`package_engine_mismatch`).
- *Dosya → Paketler...* kurulu olanları listeler ve klasöründen paket kurar: *Projeye yükle* (projeyle birlikte taşınır; `packages/` ve `wish.lock` hangi dosyaların olduğunu tam olarak kaydeder) ya da *Benim için yükle* (bu bilgisayarda açtığınız her proje için).
- *Kaldır* paketi çöp kutusuna taşır; hiçbir şey silinmez.
- Projenin kilidi denetlenir: bir paketin dosyaları değişirse pencere bunu söyler; değişiklik sizinse *Kilidi yeniden yaz*.
- **Paket dizini:** *Paket dizininden* bölümünde dizin klasörünü yazın (paketlerden oluşan bir klasör; hatırlanır), birkaç sözcük yazıp *Ara*'ya basın. Her paket, bu motorla çalışan en yeni sürümünü gösterir: *Projeye ekle*, *Benim için ekle* ya da daha yeni uyumlu bir sürüm varsa *Güncelle*. Hub'ın *Paketler* sekmesinde kendi paketleriniz için aynı arama vardır.
- Komut satırında: `wen pkg search dark theme --index <klasör>`, `wen pkg add <id>`, `wen pkg update`, `wen pkg remove <id>`, `wen pkg lock` (kendi paketleriniz için `--user`; `WISH_PACKAGE_INDEX` her seferinde `--index` yazmaktan kurtarır).

**Motor sürümleri:** motorun birden çok sürümü yan yana durabilir. Hub'ın *Motor sürümleri* sekmesi onları listeler; *Motor ekle* başka bir motorun klasörünü olduğu yerde hatırlar (`wen` ve `wish-engine.json` içeren klasör; hiçbir şey kopyalanmaz), *Kaldır* birini unutur. Bir proje, yapıldığı sürümle açılır (`wish.json` hangisi olduğunu söyler); o sürüm bilgisayarda yoksa onu değiştirmeden açabilen en yeni sürümle (her motor, hangi sürümden itibaren projeleri dosyalarını değiştirmeden açtığını bilir); o da yoksa bu motor açar ve nedenini söyler: daha yeni bir proje kendi sürümünü ister; bir dosya biçimi değişmeden önce yapılmış eski bir projenin dosyaları ise kaydedilince değişir. Son projeler listesi, başka bir sürümle yapılmış projeyi gösterir. Komut satırında: `wen engines`, `wen engines add <klasör>`, `wen engines pick <proje>`.

**Temalar:** *Görünüm → Tema* editörün görünümünü hemen değiştirir: Atelier (varsayılan), Midnight, High contrast ve paketlerin getirdiği her tema.

**Çalışma alanları:** yapay zekâ modunda oyun kartının üstündeki *Çalışma alanı* kutusu kartın ne için olduğunu seçer. *Hikâye oyunu* mekânı, karakterleri, tonu, seçimleri ve sonları sorar ve kendi hazır isteklerini getirir; *Varsayılan* motorun her oyun için kartıdır. Geçiş yaptığınızda yazdığınız hiçbir şey kaybolmaz.

**Komut paleti:** Ctrl+K ile açılır. Ne istediğinizin birkaç harfini yazın (*dışa aktar*, *yapay zekâ*, *test*), ok tuşlarıyla seçin, Enter çalıştırır. Kısayollar: Ctrl+P oynat/durdur, Ctrl+Z/Ctrl+Y geri al/yinele, Ctrl+M elle/yapay zekâ modu, Ctrl+B kodu derle. Birini değiştirmek için tercih dosyanıza ekleyin: `"keys": { "mode.switch": "Ctrl+J" }`.

## 8. Oyunu dışa aktarma

*Dosya → Oyunu dışa aktar* (ya da `wen export`):
- Önce proje denetlenir.
- Oyun kodu derlenmiş olmalıdır.
- Sonuç `exports/<ad>/` klasörüdür:
  - Windows'ta `<ad>.exe`, Linux'ta `<ad>`: oynatıcı;
  - `game/`: sahneler, varlıklar ve derlenmiş kod;
  - `LICENSES.txt`.

Oyunu paylaşmak için bu klasörü sıkıştırın. Oynatıcı içindedir; motora, editöre ya da bir
derleyiciye ihtiyaç duymaz. Editörde oynadığı gibi oynar: aynı sayıda tick'ten sonra aynı dünya.
Oyun, yapıldığı sistem için dışa aktarılır (Windows ya da Linux); aynı bilgisayardan öbür sistem
için dışa aktarmak sonraki bir sürümde gelecek.

## 9. Komut satırı

`wen` editörün yaptığı her şeyi yapar. Birkaç kullanışlı komut:

```bash
wen new oyunum --template islands
```

2B bir proje: `wen new oyunum --kind 2d` (boş) ya da `wen new oyunum --template bricks`.
`--kind` verilmezse şablonun kendi türü kullanılır, boş proje 3B olur.

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
| *This file is format 1; this engine writes format 2* | Bir sorun yok: aşağıya bakın |

**Eski bir sürümden kalan projeler.** Sahne ve prefab dosyaları bir biçim numarası taşır. Bu sürüm
biçim 2 yazar; bir sahnenin varsayılan görünümü de onunla değişti. Biçim 1 olan bir dosya olduğu
gibi açılır, eskisi gibi görünür ve oynanır: motor onun için eski değerleri korur. *Sorunlar* (ve
`wen check`) bu dosyaları bir uyarıyla listeler. Dosya, o değerler içine yazılarak biçim 2 olarak
yeniden yazılır: sahneyi editörde ilk değiştirdiğinizde, ya da hepsi birden şu komutla:

```bash
wen migrate --root oyunum
```

Böyle bir sahneye sonradan yeni görünümü vermek için *Görünüm uygula...* ile *Doğal*'ı seçin.
Dosyalar yeniden yazılmadan önce kaydedilmiş tekrarlar artık uymaz; yeniden kaydedilmeleri gerekir.

Motorun tanı iletileri İngilizcedir (ajanlar ve belgeler için tek dil); editörün arayüzü sizin
dilinizdedir.

## 11. Proje klasöründe neler var

```text
wish.json             proje: ad, tür (2B ya da 3B), varsayılan sahne
scenes/               sahneler (*.scene.json) ve prefab'lar (*.prefab.json)
assets/               modeller (.glb/.gltf), resimler (.png), sesler (.wav/.ogg),
                      malzeme dosyaları (*.mat.json), sprite sayfaları (*.sheet.json)
scripts/src/lib.rs    oyun kodu
input.json            eylemler ve tuşları
tests/                senaryo testleri (*.test.json) ve altın resimler
replays/              kaydedilmiş tekrarlar
exports/              dışa aktarılan oyunlar (projenin parçası değil)
.wish/                motorun ürettikleri (derlemeler, denetim noktaları); paylaşılmaz
.mcp.json, CLAUDE.md  bir yapay zekâ ajanının nasıl bağlanıp çalışacağı
```

Her dosya kanonik JSON'dur; git sürümler arasındaki farkı açıkça gösterir.

## 12. Bu kılavuzdaki iddialar nasıl doğrulanır

Bu komutların her biri motorun kaynak kodunun bir kopyasında çalışır (`cargo test` olanlar CI'da
Windows ve Linux'ta da çalışır).

| İddia | Komut |
|---|---|
| 12 dil | `wen edit --lang xx` hata iletisinde hepsini sayar |
| 34 araç, her birinin elle yapılış yolu | bilinmeyen bir araç adıyla `wen call` hepsini listeler; `cargo test -p wish_editor --test parity` |
| Aynı sahne, tohum ve tuşlar aynı biçimde oynar; 10.000 tick'lik tekrarlar birebir oynar | `cargo test -p wish_control --test script_tools` |
| Dışa aktarılan oyun editördeki gibi oynar | `cargo test -p wish_player --test export` (ayrı bir süreçte aynı dünya özeti) |
| Biçim 1 olan bir dosya eskisi gibi görünür ve oynar; `wen migrate` anlamını korur | `cargo test -p wish_control --test look_lock` |
| Bir sürüm arşivinin istedikleri | sürüm iş akışı her arşivi derler (Linux arşivini Ubuntu 22.04'te; bir program 2.35'ten yeni bir glibc isterse başarısız olur) ve açılmış kopyasından, makinenin Rust araçları olmadan, eksiksiz bir GitHub makine görüntüsünde kullanır: bu, arşivin geliştirici aracı istemediğini kanıtlar, çıplak bir sistemde yüklediği her kitaplığın bulunduğunu değil ([.github/workflows/release.yml](../../../.github/workflows/release.yml)); Visual C++ çalışma zamanı notu: [docs/design/M12-release-pipeline.md](../../design/M12-release-pipeline.md), bölüm 6 |
