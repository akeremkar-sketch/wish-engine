# Wish Engine — Vizyon

> **Wish Engine** — "dile, olsun". Komut satırı aracı `wen` (**W**ish **En**gine), crate öneki `wish_`.

## Tek cümle

Yapay zekâya **tamamen entegre**; arayüzü yapay zekâyı en etkili biçimde yönetmek üzerine kurulmuş ama yapay zekâ olmadan da eksiksiz kullanılabilen (**hibrit**); MCP'nin eklenti değil omurga olduğu; deterministik, metin tabanlı, modern ve **tamamen ücretsiz** bir 3D oyun motoru. Öncelik Claude; MCP konuşan diğer yapay zekâlar da aynı yüzeyi kullanır.

## Ürün vizyonu

- **Açık kaynak.** MIT + Apache-2.0. Herkes kodu okuyabilir, katkı verebilir; herkese açık kod, yapay zekâ modellerinin motoru zamanla daha iyi tanımasını da sağlar.
- **Ücretsiz ve komisyonsuz.** Motor ücretsizdir. Motorla yapılan oyunlardan gelir payı, lisans ücreti ya da abonelik alınmaz.
- **Dağıtım.** Steam ve kendi web sitemiz.
- **Yapay zekâ kullanıcıya aittir.** Kullanıcı kendi Claude (ya da başka bir sağlayıcı) hesabını bağlar. Motor araya girmez, yapay zekâ kullanımından pay almaz.
- **Hibrit.** Yapay zekâyla da, günümüz motorlarındaki gibi tamamen elle de çalışılır; ikisi aynı sahnede karışık kullanılır. Yapay zekâ bağlamak isteğe bağlıdır.
- **Sıfır kurulum.** İndir → aç → yapay zekânı bağla (ya da bağlamadan elle başla) → oyun yapmaya başla. Kullanıcıdan derleyici ya da ek araç kurması istenmez.
- **Uçtan uca.** Fikirden, dağıtılabilir oyun dosyasına (dışa aktarma) kadar her adım motorun içindedir.
- **"Sorunsuz" bir kalite çıtasıdır.** Bu belgenin sonundaki ölçütlerle ölçülür; sürüm bu ölçütler sağlanmadan çıkmaz.

## Çözdüğümüz problem

Bugünkü motorlarda (Unity, Unreal, Godot) MCP entegrasyonları, ajan düşünülmeden tasarlanmış bir editörün üstüne sonradan eklenir. Ajanın yaşadığı sorunlar hep aynı köke dayanır:

| Sorun | Kök neden |
|---|---|
| "Değişikliğim oldu mu?" belirsizliği | Durum editörün içinde gizli, sorgulanamıyor |
| Aynı hata iki kez üretilemiyor | Simülasyon deterministik değil |
| Alan adlarını tahmin etme, yanlış API çağrıları | Motor kendi şemasını makineye okunur biçimde yayınlamıyor |
| Sahne dosyaları düzenlenemiyor | Binary ya da GUID dolu, diff'lenemeyen formatlar |
| Ajanın yazdığı hatalı kod editörü çökertiyor/donduruyor | Oyun kodu motorla aynı süreçte, korumasız çalışıyor |
| Her deneme dakikalar sürüyor | Derle → yeniden başlat döngüsü |
| "Bitti" deniyor ama çalışmıyor | Doğrulama araçları (test, görsel karşılaştırma) ajana açık değil |

Wish Engine bu kök nedenleri **çekirdek tasarım değişmezleri** olarak çözer; sonradan yamayla değil.

## Hibrit: yapay zekâyla ya da elle

Motor iki biçimde de eksiksiz kullanılır ve kullanıcı aralarında istediği an geçer:

- **Yapay zekâyla:** insan niyetini söyler, gösterir, sonucu inceler, onaylar ya da geri alır; ajan (Claude ve MCP konuşan diğer yapay zekâlar) işi yapar.
- **Elle:** günümüz motorlarındaki gibi — nesneyi tut, taşı, değerini değiştir, oynat. Yapay zekâ bağlı olmasa da her şey çalışır.

İkisi rakip değil, tamamlayıcıdır: küçük ve görsel ayarlar elle daha hızlıdır, büyük ve tekrarlı işler yapay zekâyla. Elle yapılan değişiklikler de aynı geçmişe girer; ajan bunları görür ve kaldığı yerden devam eder.

**Yapay zekâ tarafı.** Arayüz klasik bir editörün içine sohbet penceresi eklenmiş hâli **değildir**; yapay zekâyı yönetme ögeleri baştan tasarımın parçasıdır (**kokpit**):

| Kokpit ögesi | Ne sağlar |
|---|---|
| Görünüm + konuşma yan yana | İstek yazılırken sahne görünür; yanıtlar sahnede canlı izlenir |
| "Göster ve söyle" | Sahnede bir nesneye, bölgeye ya da zaman aralığına tıklamak onu isteğe bağlam olarak ekler ("**şunu** biraz büyüt") |
| Etkinlik akışı | Ajanın her değişikliği bir kart: ne değişti (fark), neden, sonucu. Kart başına tek tıkla geri al |
| Otomatik denetim noktaları | Her ajan görevinden önce anlık görüntü; "yapay zekâ buna başlamadan önceki hâle dön" |
| Önce/sonra karşılaştırma | Sahnenin ve oyunun görsel farkı; değişikliği kabul et / reddet |
| Yetki kipleri | "Yalnızca öner" · "Uygula, ben incelerim" · "Tam özerk" |
| Canlı gözlem | Ajanın yürüttüğü denemeler, aldığı görüntüler ve testler izlenebilir |

**Elle taraf.** Klasik editör araçları eksiksizdir ve birinci sınıftır: görünüm penceresi, hiyerarşi, denetçi (her bileşen için otomatik üretilir), taşıma/döndürme/ölçekleme tutamaçları, varlık tarayıcı, materyal ve prefab düzenleme, oynat/duraklat/adım at/geri sar denetimleri, konsol ve performans panelleri. Oyun kodu, dış bir kod düzenleyiciyle elle de yazılabilir.

İnsan da ajan da **aynı kontrol düzlemi** üzerinden çalışır. Ajanın yapabildiği her şey elle, elle yapılabilen her şey MCP'den ve komut satırından yapılabilir. İkisi aynı sahneyi aynı anda düzenleyebilir.

## Tasarım ilkeleri (değişmezler)

Her ilke test edilebilir olacak şekilde yazılmıştır. Bir özellik bunlardan birini çiğniyorsa tasarım yanlıştır.

1. **Şema önce.** Dünyadaki her veri türü (bileşen, kaynak, olay) çalışma zamanında sorgulanabilir bir şemaya sahiptir. Şemasız veri dünyaya giremez. Ajan hiçbir alan adını tahmin etmez.
2. **Her şey metin.** Sahne, prefab, materyal, girdi haritası, test — hepsi kanonik biçimlendirilmiş, diff'lenebilir metin dosyasıdır. Dosyalar tek doğruluk kaynağıdır.
3. **Deterministik simülasyon.** Aynı derleme + aynı sahne + aynı tohum + aynı girdiler = bit düzeyinde aynı dünya. Her tick'in bir dünya özeti (hash) vardır.
4. **Gözlemlenebilirlik.** Dünyanın her parçası sorgulanabilir: varlıklar, bileşenler, olaylar, günlükler, performans, ekranda ne göründüğü (yalnızca piksel değil, anlamsal olarak da).
5. **Geri alınabilirlik.** Her düzenleme atomik bir işlemdir ve geri alınabilir. Her simülasyon anı kaydedilip geri yüklenebilir.
6. **Çökmeyen döngü.** Ajanın yazdığı oyun kodu korumalı alanda (Wasm) çalışır. Hatalı kod motoru çökertmez, dondurmaz; yapılandırılmış bir tanı mesajı üretir.
7. **Hızlı döngü.** Değişiklik → gözlem süresi saniyelerle ölçülür. Oyun mantığı değişince motor yeniden derlenmez.
8. **Tek kontrol düzlemi.** MCP, komut satırı ve editör aynı iç API'nin üç yüzüdür. Hiçbirinin diğerinde olmayan özel yeteneği yoktur.
9. **Kanıtla bitir.** Motor, "çalışıyor" iddiasını kanıtlayacak araçları (senaryo testi, görsel karşılaştırma, hash doğrulama) ajana verir.
10. **Ölç, varsayma.** Motorun ajan-dostu olduğu iddiası, otomatik ajan değerlendirme takımıyla (evals) sürekli ölçülür.
11. **Hibrit eşlik.** Ajanın yapabildiği her sahne/varlık işlemi elle de yapılabilir. Motor, hiçbir yapay zekâ bağlı değilken eksiksiz çalışır. Elle ve ajanla yapılan değişiklikler tek bir geçmişte tutulur.

## Fark yaratan özellikler

- **Motorun içinde yerleşik MCP sunucusu** — `wen new` ile oluşturulan her projede Claude Code otomatik bağlanır.
- **Zaman yolculuğu** — anlık görüntü al, geri sar, dünyayı çatalla ("şunu denesem ne olur?"), tekrar oynat.
- **Anlamsal görüş** — ekran görüntüsünün yanında "hangi varlık ekranın neresinde, yüzde kaçı görünüyor" bilgisi.
- **Dünya denetçisi (linter)** — "varlık dünyadan düştü", "sahnede ışık yok", "kamera hiçbir şey görmüyor" gibi sezgisel uyarılar.
- **Girdi enjeksiyonu** — ajan oyunu gerçekten oynayabilir; girdiler kaydedilip tekrar oynatılabilir.
- **Kayıpsız sıcak yeniden yükleme** — oyun durumu Wasm modülünde değil dünyada yaşar; kod değişince oyun kaldığı yerden devam eder.
- **Dış üretim araçlarına açık varlık hattı** — Blender MCP ya da üretken araçlar `assets/` klasörüne dosya bırakır; motor içe aktarır, doğrular, raporlar.

## Kapsam: 1.0 hedefi

"Küçük ve orta ölçekli 3D oyunları yapay zekâyla, elle ya da ikisiyle birlikte baştan sona sorunsuz yapabileceğiniz en iyi motor."

- 3D öncelikli (2D, ortografik kamera + sprite ile alt küme olarak)
- Windows masaüstü (Linux/macOS aynı koddan); web sonraki kilometre taşı
- PBR render, gölgeler, glTF içe aktarma, iskelet animasyonu
- Fizik, ses, girdi, oyun içi arayüz
- Wasm tabanlı oyun mantığı (ilk dil: Rust)
- MCP + komut satırı + hibrit editör (yapay zekâ kokpiti ve tam elle düzenleme)
- Oyunu dağıtılabilir pakete dışa aktarma
- Tek paket kurulum (Steam ve web sitesi), Türkçe ve İngilizce arayüz

## Kapsam dışı (1.0 için bilinçli olarak)

- Unreal düzeyinde AAA render (sanal geometri, ışın izlemeli küresel aydınlatma) → 1.0 sonrası araştırma hattı. Mimari buna kapı açık bırakır, ama söz vermez.
- Çok oyunculu/ağ → 1.0 sonrası. Determinizm ve anlık görüntü altyapısı bunun temelini hazırlar.
- Konsol ve mobil platformlar.
- Görsel (düğüm tabanlı) betikleme.
- Uzman elle düzenleme araçları: animasyon zaman çizelgesi düzenleyicisi, görsel gölgelendirici grafiği, arazi şekillendirme, motorun içinde tam kod düzenleyici → 1.0 sonrası. O zamana dek dış araç (Blender, VS Code), metin dosyası ya da yapay zekâ kullanılır.
- Motorun içine gömülü üretken yapay zekâ modelleri (dış araçlarla bütünleşiriz).

Bu sınırlar dürüstlük içindir: "yeni nesil motor" projelerinin başarısız olma nedeni neredeyse her zaman kapsamdır. Önce dar ama kusursuz, sonra geniş.

## Başarı ölçütleri

| Ölçüt | Hedef |
|---|---|
| Sahne değişikliği → gözlem | < 2 saniye |
| Oyun kodu değişikliği → çalışan oyun | < 10 saniye |
| Determinizm | 10.000 tick tekrar oynatmada sıfır hash sapması |
| Ajan değerlendirme takımı | Sonnet 5.5 · orta efor ile ≥ %90 görev başarısı |
| Sıfırdan oynanabilir mini oyun | Tek ajan oturumu, insan müdahalesi yok |
| Yapay zekâsız kullanım | Hiçbir yapay zekâ bağlı değilken sahne kurma, oynatma, test ve dışa aktarma eksiksiz çalışır |
| Elle/ajan eşliği | Kontrol düzlemindeki her sahne/varlık işleminin arayüzde elle karşılığı vardır (otomatik denetlenir) |
| İlk çalıştırma | İndirme sonrası: yapay zekâyı bağla → ilk istek → çalışan sahne < 5 dakika |
| Kurulum | Tek paket; kullanıcı derleyici ya da ek araç kurmaz |
| Ajan kaynaklı çökme/donma | Sıfır (hatalı oyun kodu yalnızca tanı üretir) |
| Geri alınabilirlik | Ajanın yaptığı her değişiklik arayüzden tek tıkla geri alınır |
| Dışa aktarma | Tek komut/tek tıkla çalışan, bağımsız oyun paketi |
