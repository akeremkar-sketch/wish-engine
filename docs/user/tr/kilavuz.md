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

Wish Engine klasörünü istediğiniz yere açın ve `wen edit`'i (ya da onu başlatan kısayolu)
çalıştırın. Başka bir şey kurmanız gerekmez: oyun kodunun derleyicisi klasörün içinde gelir.

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
| Orta | Sahne. Tıklayınca seçer; tutamacı sürükleyince taşır, döndürür ya da ölçekler. |
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

## 7. Oyunu dışa aktarma

Proje menüsü, *Oyunu dışa aktar* (ya da `wen export`):
- Önce proje denetlenir.
- Oyun kodu derlenmiş olmalıdır.
- Sonuç `exports/<ad>/` klasörüdür:
  - `<ad>.exe`: oynatıcı;
  - `game/`: sahneler, varlıklar ve derlenmiş kod;
  - `LICENSES.txt`.

Oyunu paylaşmak için bu klasörü sıkıştırın. Motoru, editörü ya da derleyicisi olmayan bir
makinede çalışır ve editörde oynadığı gibi oynar.

## 8. Komut satırı

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

## 9. Bir şey ters gidince

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

## 10. Proje klasöründe neler var

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
