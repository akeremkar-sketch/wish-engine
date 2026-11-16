# Sürtünme günlüğü

M7 yöntemi (bkz. [design/M7-vitrin-oyunu.md](design/M7-vitrin-oyunu.md) §3): motorla oyun yaparken takılınan her yer buraya yazılır ve oyunda değil motorda düzeltilir. Aynı takılma ikinci kez yaşanmamalı; her düzeltme bir test ya da bir değerlendirme göreviyle korunur.

Türler: **araç** (eksik araç ya da parametre), **ileti** (hata iletisi/ipucu yetersiz), **belge** (yanlış ya da eksik belge), **hata**, **API** (oyun kodu SDK'sında boşluk), **başarım**.

| No | Ne oldu | Tür | Etkisi | Düzeltme | Durum |
|---|---|---|---|---|---|
| F1 | `wen do`'ya okunaklı (çok satırlı) JSON çağrılar verdim; "Line 2 is not valid JSON" dedi. Belge "dizi ya da satır başına bir nesne" diyordu, ileti doğru yeri gösterdi. | araç | 1 deneme | `wen do` artık arka arkaya yazılmış JSON nesnelerini (satırlara yayılmış da olsa) okuyor | düzeltildi |
| F2 | Yeni projenin `CLAUDE.md`'si fizik, ses, arayüz, parçacık, 2D, animasyondan hiç söz etmiyor; bir ajan bunların varlığını ancak `docs_search` ile tahmin ederek bulur. | belge | (tahmini) birkaç arama | `wen new`'in yazdığı `CLAUDE.md`'ye "Game systems" bölümü: her sistemin bileşenleri ve gözlem aracı | düzeltildi |
| F3 | Oturum açıkken `entity_get` sahne dosyasını okudu (oyuncu hiç kıpırdamamış göründü); `sim_step` ise tek açık oturumu kendiliğinden kullanıyor. Tutarsızlık bir koşuyu boşa harcattı. | ileti | 1 koşu | Oturum açıkken `session` verilmeden okuyan `entity_get`/`entity_query` yanıtına `scene_not_session` notu (açık oturumlar ve tick'leri); proje kılavuzunda da yazıyor | düzeltildi |
| F4 | Karakter düz zeminde yürürken her birkaç tick'te `CollisionStarted`/`CollisionEnded` geldi (40 tick'te 11 çift): denetleyici kapsülü zeminden ince bir boşlukla tutuyor, temas başlayıp bitiyordu. Temas olaylarına güvenen oyun kodu için gürültü. | hata | — | Karakter çarpıştırıcılarına denetleyici boşluğunun iki katı temas payı (`contact_skin`); test `a_walking_character_touches_the_ground_once` | düzeltildi |
