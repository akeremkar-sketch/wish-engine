# Wish Engine betası: testçiler için kılavuz

Wish Engine'i 1.0 sürümünden önce denediğiniz için teşekkürler. Beta iki
hafta sürer. Bulduklarınız 1.0'dan önce neyin düzeltileceğini belirler; sizi
şaşırtan bir şey hakkında kısa bir not, bir çökme raporu kadar değerlidir.

Bu kılavuzun İngilizcesi: [../../release/beta.md](../../release/beta.md).

## 1. Gerekenler

- **Windows 10 ya da 11** (64 bit, x86-64) veya glibc 2.35 ya da daha yenisi
  olan **Linux** (x86-64; Ubuntu 22.04, Debian 12 ya da sonrası).
- Yaklaşık 1 GB boş disk alanı: açılmış motor yaklaşık 560 MB'dır (oyun
  kodu için kendi derleyicisini taşır), projeleriniz için de yer gerekir.
- Ekran kartı şart değildir; yeni bir kart editörü daha akıcı yapar.
- Yapay zekâ yardımcısı **isteğe bağlıdır**. Her şey elle yapılabilir.
  [Claude Code](https://claude.com/claude-code) kurulu ve oturum açılmışsa
  editör onu sizin hesabınızla kullanabilir; aksi hâlde hiçbir yere bir şey
  gönderilmez.

## 2. İndirme ve kurulum

1. Deponun
   [Sürümler (Releases) sayfasını](https://github.com/akeremkar-sketch/wish-engine/releases)
   açın ve sisteminize uygun beta arşivini alın:
   `wish-engine-<sürüm>-windows-x86_64.zip` ya da
   `wish-engine-<sürüm>-linux-x86_64.tar.gz`, bir de `SHA256SUMS`.
2. İndirmeyi doğrulayın (isteğe bağlı, önerilir). Windows'ta PowerShell'de:
   `Get-FileHash .\wish-engine-<sürüm>-windows-x86_64.zip` ve çıkan değeri
   `SHA256SUMS` içindeki satırla karşılaştırın. Linux'ta:
   `sha256sum -c SHA256SUMS --ignore-missing`.
3. Arşivi kendinize ait bir klasöre açın (örneğin `Belgeler\Wish Engine`).
   Sisteme hiçbir şey kurulmaz.
4. Başlatın:
   - Windows: `wish-engine.exe` dosyasına çift tıklayın. Program henüz
     imzalı değil; Windows "Bilgisayarınız korundu" diyebilir: *Ek bilgi*,
     ardından *Yine de çalıştır*'ı seçin.
   - Linux: açtığınız klasörde `./wen edit` çalıştırın.

## 3. Neler denenir (yaklaşık bir saat)

İstediğiniz kadarını, istediğiniz sırayla yapın. Duraksadığınız yerleri not
edin.

1. **Başlangıç.** Açılışı izleyin, sonra başlangıç ekranında **Islands**
   şablonundan bir proje yapıp *Oynat*'a basın (Ctrl+P). Kazanabiliyorsanız
   kazanın.
2. **Elle.** Yeni bir sahne yapın (*Dosya → Yeni sahne*), *Oluştur*
   menüsünden bir küp ekleyin, tutamaçla taşıyın, çoğaltın (Ctrl+D),
   etrafa bakın (Alt ve sol tuş döndürür, tekerlek yakınlaştırır, F seçileni
   çerçeveler), denetçide bir alanı değiştirin, geri alıp yineleyin.
3. **Oyun kodu.** *Oyun → Kodu düzenle* `scripts/src/lib.rs` dosyasını
   açar; bir sayıyı değiştirip kaydedin, sonra *Oyun → Kodu derle*
   (Ctrl+B) ve yeniden oynatın.
4. **Yapay zekâyla (isteğe bağlı).** Araç çubuğunu *Yapay zekâyla*'ya
   alın, oyun kartının birkaç satırını doldurun ve ajandan küçük bir
   değişiklik isteyin. Her görev geri alınabilir.
5. **Dışa aktarma.** *Dosya → Oyunu dışa aktar* `exports/` altında motor
   olmadan çalışan bir klasör yapar; oyunu oradan başlatın.

Kullanım kılavuzu her bölümü anlatır: [Türkçe](guide.md),
[English](../en/guide.md).

## 4. Nasıl bildirilir

Deponun
[Issues sayfasında](https://github.com/akeremkar-sketch/wish-engine/issues/new/choose)
bir kayıt açın ve **Beta feedback**'i (her şey: kafa karıştıran, yavaş,
çirkin, eksik) ya da **Bug report**'u (bir şey ters gitti) seçin. Türkçe
yazabilirsiniz. Lütfen şunları yazın:

- ne yaptığınız, ne beklediğiniz ve ne olduğu;
- sisteminiz (Windows ya da Linux sürümü) ve biliyorsanız ekran kartınız;
- gördüğünüz bir şeyle ilgiliyse bir ekran görüntüsü;
- çökme olduysa çökme klasöründeki en yeni dosya. Çökme raporları siz
  ekleyene kadar bilgisayarınızda kalır; kendiliğinden hiçbir şey
  gönderilmez.
  - Windows: `%LOCALAPPDATA%\Wish\crashes` (editör hiç açılmadıysa
    `%LOCALAPPDATA%\Wish\launcher-error.txt` de)
  - Linux: `~/.config/wish/crashes` (ya da `$XDG_CONFIG_HOME/wish/crashes`)

## 5. Bu betadaki bilinen eksikler

- **Ses** çalışıyor ama henüz her sistemde kulakla denenmedi; bir oyun
  sessizse bildirin.
- **Windows:** oyun kodunu derlemek için Microsoft Visual C++
  Redistributable (`VCRUNTIME140.dll`) gerekir; çoğu Windows'ta zaten
  vardır. *Kodu derle* bunun eksik olduğunu söylerse
  [Visual C++ Redistributable](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist)
  paketini kurup yeniden deneyin.
- **macOS** 1.0'da yok.
- Program henüz kod imzalı değil (yukarıdaki Windows uyarısına bakın).
- 1.0'da henüz yok, sonraki sürümlerde planlı: kayıtlı oyunlar ve oyun
  sırasında sahne geçişi, varlıkların resimli kutular olarak gösterilmesi,
  web'e dışa aktarma.

Projeleriniz sıradan metin dosyası klasörleridir: sonraki bir derleme birini
açamazsa onu saklayın ve bize söyleyin.
