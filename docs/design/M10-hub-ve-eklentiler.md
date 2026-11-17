# M10 — Hub, paketler ve modlanabilirlik (plan)

**Durum:** plan, 17 Kasım 2026; kullanıcı onayı bekliyor.
**Önerilen model · efor:** M10a ve M10c Opus 5.5 · xhigh (kalıcı arayüz kararları); M10b, M10d ve M10e Opus 5.5 · high.
**Kapsam uyarısı:** Bu plan 1.0 kapsamını büyütür. Öneri §9'da: temel altyapı (M10a) ve Hub (M10b) 1.0'a girsin; kodlu eklentiler (M10c), oyun paketleri (M10d) ve ekosistem (M10e) 1.x sürümlerinde gelsin.

## 1. İstek

- Yapay zekâ modu bir **temel şablon** olsun; insanlar onu kolayca değiştirip kendi çalışma alanlarını yapabilsin.
- Unity Hub ve Epic Launcher gibi bir **Hub** olsun: projeler, motor sürümleri, eklentiler, modlar.
- Arayüz ve daha birçok şey modlanabilsin, çok işlevli olsun.
- Bunların altyapısı şimdiden, erken kurulsun.

## 2. Neden şimdi

Eklenti altyapısı sonradan eklendiğinde motorun kendi parçaları "özel" kalır: eklentiler ikinci sınıf olur, iç API'ler dışarı sızar, uyumluluk kırılır. Erken kurmanın asıl değeri **motorun kendi özelliklerinin de aynı kayıtlardan geçmesi**dir: yapay zekâ modu, temalar, araçlar, şablonlar birer "birinci taraf eklenti" olur. Böylece:

- eklenti yazarları motorun kullandığı API'nin aynısını kullanır;
- API'nin eksikleri motoru yaparken görünür;
- motor değiştirmeden modlanabilir hale gelir.

Bugün birçok şey zaten veri: diller (`lang/*.json`), şablonlar (`examples/*`), tarifler (`recipes.md`), tanı kataloğu, araç kataloğu. Plan bunları tek bir **paket** kavramı altında toplar.

## 3. Değişmezlerle uyum

| Değişmez | Eklentiler için anlamı |
|---|---|
| 2 Determinizm | Paket sürümleri kilit dosyasıyla sabit (`wish.lock`, hash'li); tekrar kaydı paketlerin parmak izini taşır. Oyuna giren kod yine belirlenimci Wasm. |
| 4 Tek kontrol düzlemi | Eklenti yeni bir yeteneği `wish_control`'e **araç olarak** kaydeder; arayüz, MCP ve komut satırı onu aynı yerden görür. Yalnızca arayüzde yaşayan eklenti yeteneği olmaz. |
| 5 Tek tanı formatı | Eklentilerin hataları da `code`, `message`, konum ve `hint` taşır; kod öneki paketin adıdır. |
| 7 Güvenilmez kod | Kodlu eklenti Wasm'da, oyun koduyla aynı korumalı alanda (yakıt, bellek sınırı) çalışır. Dosya, ağ ve proje erişimi yalnızca manifestte istenen ve kişinin onayladığı **yetkilerle** olur. Yerel (native) eklenti yok. |
| 9 Hibrit eşlik | Eklentinin eklediği her araç yapay zekâyla da elle de kullanılabilir; eklentinin paneli araçlarını çağırır. |
| Lisans | Her paket SPDX lisansını bildirir; Hub, lisansı izin verilen listeyle (`deny.toml` gibi) karşılaştırıp gösterir. |

## 4. Paket modeli

Her şey bir **paket**: `wish-package.json` manifestli bir klasör (ya da onun `.zip`'i).

```json
{
  "format": 1,
  "id": "com.example.narrative-workspace",
  "name": "Narrative Workspace",
  "version": "1.2.0",
  "engine": ">=1.0, <2.0",
  "license": "MIT",
  "kinds": ["workspace", "recipes", "theme"],
  "capabilities": ["project.read", "design.write"],
  "dependencies": { "com.example.dialogue-kit": "^0.3" },
  "contents": { "workspaces": ["workspaces/story.json"], "themes": ["themes/ink.json"] }
}
```

**Paket türleri** (zararsızdan güçlüye):

| Tür | Ne | Kod mu | Örnek |
|---|---|---|---|
| `theme` | Renk belirteçleri, yazı tipleri, simge seti (SVG yolları), köşe ve boşluk ölçüleri | Hayır | "Gece mavisi", yüksek karşıtlık |
| `language` | Arayüz çevirisi | Hayır | Japonca |
| `template` | Yeni proje şablonu (bugünkü `examples/*`) | Hayır (oyun kodu derlenir) | "Top-down RPG" |
| `workspace` | Yapay zekâ modunun düzeni: paneller, oyun kartı alanları, hazır istekler, karar ve soru hazırları, ajana ek talimat | Hayır | "Hikâye oyunu çalışma alanı" |
| `recipes` | Ajanın okuduğu tarifler ve örnekler (`wish://docs/...`) | Hayır | "Diyalog sistemi tarifleri" |
| `agent` | Ajan bağdaştırıcısı tanımı (komut satırı, argümanlar, MCP yapılandırması) | Hayır (dış program) | Başka bir MCP istemcisi, yerel model sarmalayıcısı |
| `tool` | Kontrol düzlemine yeni araç (Wasm) | Evet | "Seviye denetleyici" |
| `panel` | Editöre panel (Wasm; bildirimsel arayüz ağacı döner) | Evet | Diyalog grafiği düzenleyicisi |
| `importer` / `exporter` | Varlık içe aktarıcı, dışa aktarma hedefi (Wasm) | Evet | `.aseprite` içe aktarıcı |
| `game` | Oyun paketi: bileşenler, sistemler, prefab'lar, varlıklar | Evet (oyunla derlenir) | "Platform kiti" |

**Proje düzeyi:**
- `wish.json` → `packages: { id: sürüm aralığı }` ve `engine` sürümü.
- `wish.lock` → çözümlenmiş sürümler ve SHA-256'lar (git'e girer).
- Paketler kullanıcı önbelleğinde (`%LOCALAPPDATA%\Wish\packages\<id>\<sürüm>`); istenirse projeye kopyalanır (`packages/`, çevrimdışı ve yeniden üretilebilir).

**Editör düzeyi:** temalar, diller, çalışma alanları, ajan bağdaştırıcıları kişinin tercihlerine kurulur.

## 5. Mimari: her şey bir kayıt

Bugün sabit listeler var: `CATALOG`, `LANGUAGES`, `style.rs` sabitleri, `ai_mode::CARD`, `ready_requests()`. Bunlar **kayıtlara** dönüşür. Motorun kendi parçaları da aynı kayda yazılır.

| Kayıt | Yeri | Bugün | Sonra |
|---|---|---|---|
| Araçlar | `wish_control` | `tools::CATALOG` (sabit) | `Registry<Tool>`: motorun araçları + paketlerin Wasm araçları (ad alanlı: `paket.araç`) |
| Komutlar | `wish_editor` | Düğmelere gömülü eylemler | Kimlikli komutlar (`editor.play`, `ai.next_step`); **komut paleti (Ctrl+K)**, değiştirilebilir kısayollar; eklentiler komut ekler; ajan da komut çağırabilir |
| Paneller | `wish_editor` | `app.rs` içinde sabit yerleşim | Panel kaydı + yerleşim belgesi; yapay zekâ modu bir `workspace` belgesi olur |
| Temalar | `wish_editor::style` | Sabitler | Belirteçler (`color.accent`, `radius.widget`...) + tema JSON'u; varsayılan tema da bir paket |
| Simgeler | `vector::parts` | Sabit yollar | Simge kaydı; tema simge setini değiştirebilir |
| Diller | `lang/*.json` | Gömülü | Gömülü + paket dilleri |
| Şablonlar | `examples/*` | Klasör | `template` paketi |
| Tanılar | `docs::DIAGNOSTICS` | Sabit | Motorunkiler + paketlerinki (ad alanlı) |
| Ajan bağdaştırıcıları | `wish_agent` | Claude Code, Scripted | `agent` paketleri; Claude Code ilk paket |

**Çalışma alanı (workspace) belgesi:** yapay zekâ modunun kendisi bu dosyadır:

```json
{
  "format": 1,
  "name": "Yapay zekâyla (varsayılan)",
  "layout": {
    "left":   [{ "panel": "design.card" }, { "panel": "design.decisions" }],
    "center": [{ "panel": "view.game" }],
    "bottom": [{ "panel": "design.plan" }, { "panel": "design.notes" }, { "panel": "project.health" }],
    "right":  [{ "panel": "agent.conversation" }]
  },
  "card": [
    { "key": "title", "label": "Name" },
    { "key": "pitch", "label": "In one sentence", "lines": 2 },
    { "key": "classes", "label": "Character classes", "list": true }
  ],
  "ready": [{ "name": "Add a level", "request": "..." }],
  "agent": { "instructions": "...", "tools": ["*"], "model": null }
}
```

- Oyun kartının alanları veriden gelir. Böylece türlere özel kartlar olur: RPG için "sınıflar", hikâye oyunu için "karakterler".
- `game_design` aracı kart alanlarını çalışma alanından okur (şema önce: alanların türü ve sınırı orada).
- Kişi editörde düzeni sürükleyip değiştirebilir, kaydedebilir ve paket olarak paylaşabilir.

## 6. Hub (karşılama ekranının büyümesi)

Ayrı bir program değil, karşılama ekranının evrimi (`wish-engine.exe` açınca gelen ekran). Sekmeler:

| Sekme | İçerik |
|---|---|
| **Projeler** | Son projeler, yeni proje (şablonlar = `template` paketleri), aç, klasörde göster; her projenin motor sürümü |
| **Öğren** | Kılavuz, örnek oyunlar, tarifler, kısa görevler ("ilk oyununu 10 dakikada yap") |
| **Paketler** | Kurulu paketler; dizinden arama, kurma, güncelleme, kaldırma; lisans, yetkiler, bağımlılıklar, sürüm notları |
| **Görünüm** | Temalar, çalışma alanları, diller, simge setleri; anında önizleme |
| **Motor sürümleri** | Yan yana kurulu sürümler; proje `wish.json`'daki sürümle açılır; güncelleme (imzalı) |
| **Yapay zekâ** | Ajan bağdaştırıcıları (Claude Code ve diğerleri), hesap durumu, varsayılan model, harcama özeti |
| **Ayarlar** | Klasörler, gizlilik (hiçbir şey gönderilmez), çökme raporları |

**Paket dizini (registry), sunucusuz ve ücretsiz:**
- Açık bir git deposu (`wish-registry`), paket başına bir JSON: sürümler, indirme adresi (GitHub Releases vb.), SHA-256, lisans, yayımcının açık anahtarı.
- Hub dizini indirir, aramayı yerelde yapar. Çevrimdışı çalışır. Komisyon yok, ödeme yok (vizyonla uyumlu).
- Yayımlamak = dizine PR (`wen pkg publish` hazırlar). İmza ed25519; doğrulanmamış paket açıkça işaretlenir.
- Kişi başka dizinler de ekleyebilir (şirket içi, topluluk).

## 7. Kodlu eklentiler (M10c) ve oyun paketleri (M10d)

**Editör eklentileri (Wasm):**
- `wish_ext` ABI v1: oyun ABI'sinden ayrı, sürümlü.
- Eklenti şunları kaydeder: araçlar (şemalı parametrelerle; `Param` tablosu aynı), paneller (her karede bildirimsel bir arayüz ağacı döner: başlık, metin, düğme, alan, liste; editör onu egui'ye çevirir), içe ve dışa aktarıcılar, `check` kuralları, olay kancaları (kaydetmeden önce, görevden sonra).
- Eklenti kendi başına dosya yazamaz: kontrol düzlemi araçlarını çağırır ve bu çağrılar geçmişe `actor: "ext:<paket>"` ile girer.
- Korumalı alan `wish_script`'in aynısı (yakıt, bellek). Düşmanca paket testleri (manifest, ABI bulanıklaştırma) şart.

**Oyun paketleri:**
- Kaynak paketi: oyun koduyla aynı `rustc` derlemesine `--extern` olarak girer (ADR-025'in uzantısı).
- Kendi bileşen, sistem, prefab ve varlıklarını getirir; şemaları ad alanlıdır.
- Sürüm çözümü semver; kilit dosyası; tekrar kaydı paket parmak izini taşır.

## 8. "Motoru daha iyi yapmak" için ek stratejiler

| Strateji | Neden | Ne zaman |
|---|---|---|
| **Komut paleti ve komut kimlikleri** | Klavyeyle her şey; modlanabilir kısayollar; ajanın da komut çağırabilmesi | M10a |
| **Tema belirteçleri** | Arayüzün yeniden derlemeden değişmesi; erişilebilirlik (yüksek karşıtlık, büyük yazı) | M10a |
| **Yerleştirilebilir (dock) paneller** | Unity ve Unreal gibi kişinin kendi düzenini kurması; çalışma alanı belgesinin elle düzenlenmesi | M10a |
| **Motor sürümü sabitleme + göç** | Eski projelerin yeni motorla güvenle açılması (`wen migrate` zaten var) | M10a |
| **Git dostu birleştirme** | Kanonik JSON sayesinde sahne ve tasarım dosyaları için 3 yollu birleştirme sürücüsü (`wen merge`); ekip çalışması sunucusuz | M10b |
| **İmzalı güncelleyici** | Hub'dan tek tıkla güvenli güncelleme | M10b |
| **Linux ve macOS paketleri** | Topluluk ve eklenti yazarları için | M10b–M10e |
| **`profile_get`** | Planlanmış ama yapılmamış; eklentilerin maliyetini de ölçer | M10c |
| **Eklenti geliştirme kiti** | `wen ext new/test/pack/publish`, şablon eklenti, belgeler şemadan üretilir | M10c |
| **RFC süreci** | Eklenti API'sindeki kalıcı değişiklikler herkese açık tartışılır | M10e |

## 9. İş paketleri ve sıra

**M10a — Temel altyapı (1.0'a önerilir)**
1. Kayıtlar: araç, komut, panel, tema, simge, dil, şablon, tanı. Motorun parçaları kayda taşınır, davranış değişmez.
2. Komut paleti (Ctrl+K) ve değiştirilebilir kısayollar.
3. Tema belirteçleri ve tema JSON'u; varsayılan tema bir paket.
4. Çalışma alanı belgesi: yapay zekâ modu ve elle mod iki varsayılan çalışma alanı olur; oyun kartı alanları ve hazır istekler veriden gelir; düzen kaydedilir.
5. Paket manifesti, `wish.lock`, yerel paket klasörü; veri paketleri (tema, dil, şablon, çalışma alanı, tarif).
6. `wish.json` motor sürümü alanı.
7. Testler: her kayıt için "motorun parçası da kayıttan geçer" testi; düşmanca manifestler; eşlik.

**M10b — Hub (1.0'a önerilir)**
1. Karşılama ekranı → Hub sekmeleri (Projeler, Öğren, Paketler, Görünüm, Yapay zekâ, Ayarlar).
2. Paket dizini biçimi ve `wen pkg search/add/remove/update/lock`.
3. Kurma, kaldırma, güncelleme; lisans ve yetki gösterimi; imza doğrulama.
4. Motor sürümleri: yan yana kurulum, projeyi doğru sürümle açma.
5. Örnek paketler: bir tema, bir çalışma alanı ("Hikâye oyunu"), bir tarif paketi.

**M10c — Kodlu eklentiler (1.x)**
1. `wish_ext` ABI ve korumalı alan; yetkiler.
2. Araç, panel, içe aktarıcı ve kanca türleri.
3. `wen ext` geliştirme kiti; örnek "Seviye denetleyici".

**M10d — Oyun paketleri (1.x)**
1. Kaynak paketlerinin oyunla derlenmesi, ad alanlı şemalar.
2. Örnek "Platform kiti".

**M10e — Ekosistem (1.x)**
1. Yayımlama akışı, imza anahtarları, dizin deposu.
2. Belge sitesi, RFC süreci.

## 10. Kullanıcıya ait kararlar

- Kapsam: M10a ve M10b 1.0'a girsin mi (öneri: evet)?
- Paket dizini deposunun adı ve sahibi (GitHub hesabı); ileride ad ve marka kararıyla birlikte.
- İlk resmi paketlerin hangileri olacağı (öneri: koyu ve açık tema, "Hikâye oyunu" çalışma alanı, "Platform" tarifleri).

## 11. Riskler

- **Kapsam:** her iş paketi bir kilometre taşı büyüklüğünde. Sıra ve 1.0 sınırı korunmalı.
- **API'yi erken dondurmak:** ABI v1 "deneysel" işaretiyle çıkar; sürüm alanı ilk günden var.
- **Güvenlik:** kodlu eklenti, oyun kodu kadar güvenilmez sayılır; yetkisiz dosya ya da ağ erişimi yok; düşmanca testler kapanış incelemesinin parçası.
- **Determinizm:** paketler tekrar kaydında ve hash'te izlenir; kilit dosyası olmadan oynatma uyarı verir.
