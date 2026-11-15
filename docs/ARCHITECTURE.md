# Wish Engine — Mimari

Kararların gerekçeleri [DECISIONS.md](DECISIONS.md) içindedir; bu belge parçaların nasıl birleştiğini anlatır.
Kod örnekleri **hedeflenen kullanım biçimini** gösterir; kesin imzalar ilgili kilometre taşında netleşir.

## Katmanlar

```
┌──────────────────────────────────────────────────────────────────┐
│  İSTEMCİLER                                                      │
│  Claude / diğer ajanlar      İnsan (terminal)      İnsan (editör)│
│        │ MCP                      │ CLI                 │ egui   │
├────────┼──────────────────────────┼─────────────────────┼────────┤
│  wish_mcp                      wish_cli               wish_editor   │
│        └──────────────┬───────────┴─────────────────────┘        │
│  KONTROL DÜZLEMİ      ▼                                          │
│  wish_control — komutlar · sorgular · oturumlar · tanılar         │
├──────────────────────────────────────────────────────────────────┤
│  BELGE KATMANI                    │  ÇALIŞMA ZAMANI              │
│  wish_scene — sahne/prefab belgesi │  wish_app — döngü, oturumlar  │
│  işlem · geri al · kanonik yazım  │  wish_script — Wasm ev sahibi │
│  wish_asset — içe aktarma, izleme  │  wish_test — senaryo, altın   │
├──────────────────────────────────────────────────────────────────┤
│  ALT SİSTEMLER                                                   │
│  wish_render   wish_physics   wish_audio   wish_input   wish_ui       │
├──────────────────────────────────────────────────────────────────┤
│  ÇEKİRDEK                                                        │
│  wish_ecs — dünya, arketipler, sorgular, zamanlama, anlık görüntü │
│  wish_schema — tür şeması, JSON ⇄ ikili, JSON Schema              │
│  wish_core — kimlikler, hash, tanı türleri, matematik             │
└──────────────────────────────────────────────────────────────────┘
        Konuk tarafı (Wasm'a derlenir):  wish_sdk + oyunun kendi kodu
```

**Bağımlılık kuralı:** oklar yalnızca aşağı iner. Alt sistemler birbirini tanımaz; ECS üzerinden (bileşen ve olaylarla) konuşur. Hiçbir alt sistem kontrol düzlemini bilmez; kontrol düzlemi hepsini şema üzerinden görür.

## Crate'ler

| Crate | Sorumluluk | Kilometre taşı |
|---|---|---|
| `wish_core` | Kimlikler, kararlı hash, `Diagnostic`, matematik yeniden dışa aktarımı | M1 |
| `wish_schema` | `TypeSchema`, `#[component]` türetme makrosu, JSON ⇄ ikili, JSON Schema üretimi | M1 |
| `wish_ecs` | Dünya, arketip depolama, sorgular, komutlar, kaynaklar, olaylar, zamanlayıcı, anlık görüntü, dünya hash'i | M1 |
| `wish_app` | Ana döngü, sabit adım, eklenti kaydı, oturum yönetimi | M1 |
| `wish_cli` | `wen` ikilisi | M1 → sürekli |
| `wish_scene` | Konumlu katı JSON + kanonik yazıcı, sahne/prefab belgesi ve doğrulaması, işlemler + geri al/yinele + `actor` günlüğü, prefab açma, belge → dünya (`SceneId`) | M2 |
| `wish_asset` | Varlık dizini, `*.meta.json`, şemaya göre `AssetRef` bulma/denetleme, `wen mv`, tüm proje yükleme (`wen check/fmt`), yoklamalı izleyici + sıcak yükleme. İçe aktarıcılar ve önbellek M3'te | M2 (+M3) |
| `wish_render` | wgpu, render grafiği, materyaller, başsız yakalama, hata ayıklama görünümleri | M3 |
| `wish_window`, `wish_input` | Pencere, aygıtlar, eylem haritaları, enjeksiyon/kayıt | M3 / M5 |
| `wish_control` | Kontrol düzlemi API'si | M4 |
| `wish_mcp` | MCP sunucusu (stdio + HTTP) | M4 |
| `wish_script`, `wish_sdk` | Wasm ev sahibi, ABI, sıcak yükleme / konuk SDK'sı | M5 |
| `wish_test` | Senaryo test koşucusu, altın görüntü, tekrar | M5 |
| `wish_physics` | Rapier bütünleşmesi | M6 |
| `wish_audio` | Kira bütünleşmesi, ses olay günlüğü | M6 |
| `wish_ui` | Oyun içi arayüz | M6 |
| `wish_editor` | Yapay zekâ kokpiti (arayüz) + ajan bağdaştırıcıları | M8 |
| `wish_export` | Oyun paketleme ve dışa aktarma | M9 |

## Şema: her şeyin kaynağı

```rust
/// Bir varlığı sabit hızla hareket ettirir.
#[component]
pub struct Mover {
    /// Hareket hızı.
    #[schema(unit = "m/s", range = 0.0..=50.0, default = 5.0)]
    pub speed: f32,
}
```

Bu tek tanımdan türeyenler:

```
                        ┌─► ECS sütun yerleşimi (boyut, hizalama, ofsetler)
                        ├─► JSON okuma/yazma (sahne dosyaları, MCP yükleri)
  #[component] ─► TypeSchema ─► JSON Schema (schema_get, editör doğrulaması)
                        ├─► Wasm ABI yerleşimi (konuk tarafı aynı baytları görür)
                        ├─► Editör denetçi alanları (aralık → kaydırıcı)
                        └─► Belgeler (wish://docs/components/Mover)
```

Ajanın `schema_get("Mover")` çağrısında gördüğü: alan adı, türü, birimi, aralığı, varsayılanı ve belge yorumu.

## Dünya ve oturum modeli

```
  DİSK (doğruluk kaynağı)            BELLEK
  scenes/main.scene.json  ◄────►  Düzenleme belgesi (revizyon N, geri al yığını)
                                        │ sim_start (kopya + tohum)
                                        ▼
                                  Oynatma oturumu s1 ── anlık görüntü ──► çatal s2
                                  (dünya + fizik + RNG + tick)
```

- **Düzenleme belgesi**: sahne dosyasının bellekteki karşılığı. Değişiklikler işlemle yapılır, diske kanonik yazılır.
- **Oynatma oturumu**: belgeden türetilen canlı dünya. Deterministik ilerler. Diske yazmaz. Birden çok oturum aynı anda yaşayabilir (başsız ya da pencereli).
- Oturumdaki bir değeri kalıcı kılmak açık bir adımdır (oturumdan belgeye yama).

## Kare döngüsü

```
her kare:
  girdi topla (aygıt → eylemler)  +  enjekte edilen eylemler
  birikmiş süre ≥ sabit adım oldukça:
      ┌ FixedUpdate ─────────────────────────────────────────────┐
      │ 1. girdi eylemlerini dünyaya yaz                         │
      │ 2. oyun sistemleri (Wasm + yerel), açık sırada           │
      │ 3. ertelenmiş komutları uygula (sabit sırada)            │
      │ 4. fizik adımı                                           │
      │ 5. hiyerarşi/dönüşüm yayılımı                            │
      │ 6. olayları günlüğe yaz, dünya denetçisini çalıştır      │
      │ 7. tick++  (istenirse dünya hash'i)                      │
      └──────────────────────────────────────────────────────────┘
  render (determinizm sınırının dışı): ara değerleme → render grafiği → sunum/yakalama
```

Başsız kipte "birikmiş süre" yoktur: `sim_step(n)` tam olarak n tick ilerletir, gerçek zamandan bağımsız ve olabildiğince hızlı.

## Oyun mantığı (Wasm konuk)

```rust
use wish_sdk::prelude::*;

#[system(stage = FixedUpdate)]
fn move_player(actions: Res<Actions>, time: Res<Time>, mut q: Query<(&Mover, &mut Transform)>) {
    let dir = actions.axis2("move");
    for (mover, tf) in q.iter_mut() {
        tf.translation += vec3(dir.x, 0.0, -dir.y) * mover.speed * time.delta;
    }
}
```

Sıcak yeniden yükleme akışı:

```
scripts/src/*.rs değişti
   → derle (wasm32)  ── hata? → tanılar (dosya, satır, ipucu) → eski modül çalışmaya devam eder
   → yeni modülü doğrula (ABI sürümü, şema uyumu)
   → tick sınırında eski modülü bırak, yenisini örnekle
   → şema değiştiyse: bileşen verisini alan adına göre taşı (yeni alan = varsayılan)
   → oyun kaldığı tick'ten devam eder
```

Durum konukta yaşamadığı için kaybedilecek bir şey yoktur. Sistem bir tuzak atarsa ya da yakıt sınırını aşarsa: o sistem devre dışı kalır, tanı üretilir, dünya ilerlemeye devam eder.

## Bir düzenlemenin yolculuğu

```
edit_apply({ base_revision: 41, ops: [ spawn …, set … ] })
  1. şema doğrulaması (alan adı, tür, aralık, başvuruların varlığı)
  2. hata varsa: hiçbir şey uygulanmaz → yapılandırılmış hata + ipucu
  3. belgeye uygula → revizyon 42 → geri al yığınına ekle
  4. kanonik JSON'u diske yaz
  5. açık oynatma oturumlarına canlı yansıt (istenirse)
  6. yanıt: fark (diff) + dünya denetçisi uyarıları + revizyon
```

Doğrudan dosya düzenlemede 1–2. adımlar dosya izleyicide çalışır; hata varsa bellekteki son geçerli belge korunur ve tanı `check` ile görünür.

## Tanılar: tek format

Derleyici hatası, sahne doğrulama hatası, çalışma zamanı uyarısı, MCP hatası — hepsi aynı yapıdadır:

```json
{
  "severity": "error",
  "code": "unknown_field",
  "message": "Component 'Transform' has no field 'position'.",
  "location": { "file": "scenes/main.scene.json", "path": "/entities/player/components/Transform/position" },
  "hint": "Did you mean 'translation'?",
  "candidates": ["translation", "rotation", "scale"],
  "docs": "wish://docs/components/Transform"
}
```

Tanı kodları kararlıdır ve belgelenir. İpucu alanı zorunludur: tanı yazan kişi "nasıl düzelir" sorusunu yanıtlamak zorundadır.

## Dünya denetçisi

Her oynatma adımından ve düzenlemeden sonra çalışan ucuz sezgisel kurallar. Ajanın ekran görüntüsüne bakıp anlamaya çalışacağı şeyleri sözle söyler:

- Varlık dünyadan düştü (sınırların çok dışında)
- Dönüşümde NaN/sonsuz, sıfır ölçek
- Sahnede ışık yok / etkin kamera yok / kamera hiçbir şey görmüyor
- Çarpıştırıcılar doğuşta iç içe
- Model ölçeği olağandışı (birim uyuşmazlığı olabilir)
- Eksik varlık dosyası başvurusu

## Render hattı

```
Sahne çıkarımı (ECS → render dünyası)
   → ayıklama → ışık kümeleme → gölge geçişleri → opak (PBR) → gökyüzü → saydam
   → ardıl işlemler (ton eşleme …) → sunum | yakalama (PNG)
   yan çıktılar (istek üzerine): varlık kimliği · derinlik · normaller · tel kafes · çarpıştırıcılar
```

`describe_view` kimlik tamponunu okur: görünen her varlık için ekran kutusu, görünürlük yüzdesi, kameraya uzaklık.

## Oyun projesi düzeni

`wen new my_game` şunu üretir:

```
my_game/
  wish.json            proje bildirimi (ad, motor sürümü, giriş sahnesi, tick hızı)
  scenes/main.scene.json
  prefabs/
  assets/                 glb, png, ogg … + *.meta.json
  scripts/                oyun mantığı (Rust → Wasm)
    Cargo.toml
    src/lib.rs
  input.json              eylem haritaları
  tests/*.test.json       senaryo testleri
  CLAUDE.md, AGENTS.md    projeye özel ajan kılavuzu (motor sürümüne göre üretilir)
  .mcp.json               Claude Code'un motor MCP sunucusuna otomatik bağlanması için
  .wish/                   önbellek, anlık görüntüler, tekrarlar (git dışı)
```

Örnek sahne (biçim 1, M2'de kesinleşti; kanonik dosyada anahtarlar bayt sırasıyla yazılır, `wen fmt` bunu yapar. Kesin kurallar: [design/M2-veri-katmani.md](design/M2-veri-katmani.md) §2–§3 ve §12):

```json
{
  "format": 1,
  "entities": {
    "sun": {
      "components": {
        "Transform": { "rotation": { "euler_deg": [-45, 30, 0] } },
        "DirectionalLight": { "illuminance_lux": 100000, "shadows": true }
      }
    },
    "player": {
      "prefab": "prefabs/player.prefab.json",
      "components": { "Transform": { "translation": [0, 1, 0] } }
    },
    "player_camera": {
      "parent": "player",
      "components": {
        "Transform": { "translation": [0, 2, 6] },
        "Camera": { "fov_y_deg": 60 }
      }
    }
  }
}
```

Örnek senaryo testi:

```json
{
  "format": 1,
  "scene": "scenes/main.scene.json",
  "seed": 7,
  "steps": [
    { "input": { "move": [0, 1] }, "frames": 120 },
    { "expect": "entity('player').Transform.translation.z < -5" },
    { "capture": "after_move", "compare": "golden", "tolerance": 0.02 }
  ]
}
```

## Hibrit arayüz

Arayüz hibrittir (ADR-018): elle düzenleme ve yapay zekâ kokpiti aynı pencerede, aynı kontrol düzleminin üstünde yaşar. İkisi de yeni bir yetenek katmanı değil, çekirdeğin zaten sağladığı şeylerin görsel yüzüdür.

**Elle düzenleme araçları:**

| Araç | Dayandığı çekirdek mekanizma |
|---|---|
| Denetçi | `TypeSchema` → alanlar otomatik üretilir (aralık → kaydırıcı, enum → liste, birim → etiket); her değişiklik bir `edit_apply` işlemi |
| Tutamaçlar (taşı/döndür/ölçekle) | Sürükleme boyunca canlı önizleme, bırakınca tek `edit_apply` işlemi |
| Hiyerarşi, varlık tarayıcı | `scene_get`, `entity_query`, `asset_list` + sürükle-bırak → `edit_apply` |
| Oynat/duraklat/adım at/geri sar | `sim_start`, `sim_step`, `sim_snapshot`, `sim_restore` |
| Konsol, performans paneli | `logs_get`, `profile_get` |

**Tek geçmiş.** Her işlem kimin yaptığını (`actor`: `human` ya da ajan adı) taşır. Elle ve ajanla yapılan değişiklikler aynı revizyon zincirinde, aynı geri al yığınında ve aynı etkinlik akışında durur. Ajan, `edit_history` ile insanın elle yaptıklarını görür.

**Yapay zekâ kokpiti ögeleri:**

| Kokpit ögesi | Dayandığı çekirdek mekanizma |
|---|---|
| Etkinlik akışı, kart başına geri al | `edit_apply` işlemleri + revizyonlar + geri al yığını |
| Otomatik denetim noktaları | Belge revizyonu işareti + oturum anlık görüntüsü |
| "Göster ve söyle" | Varlık kimliği tamponu (tıklanan piksel → varlık) + `describe_view` |
| Önce/sonra karşılaştırma | Belge farkı + iki revizyondan yakalanan görüntüler |
| Yetki kipleri | Kontrol düzleminde `dry_run` + onay kapısı |
| Canlı gözlem | Oturum olay akışı (günlükler, yakalamalar, test sonuçları) |

Kokpit içi ajan, sağlayıcıdan bağımsız bir bağdaştırıcı üzerinden konuşur (ADR-024); dış ajanlar (Claude Code vb.) MCP'den bağlanır. İkisinin etkinliği aynı akışta görünür, çünkü ikisi de aynı kontrol düzlemi işlemlerini üretir.

## Dağıtım ve dışa aktarma

```
Motor paketi (Steam / web)            Dışa aktarılan oyun (wen export)
  wen çalıştırılabilir dosyası          oyun çalıştırılabilir dosyası (yalnızca çalışma zamanı)
  kokpit                                paketlenmiş varlıklar
  MCP sunucusu                          derlenmiş Wasm modülleri
  gömülü Wasm derleme araç zinciri      (editör, MCP sunucusu, derleyici yok)
  örnek projeler + belgeler
```

Kullanıcı ek araç kurmaz (ADR-023). Dışa aktarılan oyun motor kurulumuna ihtiyaç duymaz.

## İş parçacığı modeli

- **M1–M5:** simülasyon tek iş parçacıklı (önce doğruluk ve determinizm). Varlık yükleme ve render hazırlığı arka planda olabilir; simülasyon durumuna dokunmazlar.
- **Sonra:** erişimleri ayrık sistemler paralel; komut tamponları sabit sırada birleşir. Paralel ve tek iş parçacıklı çalıştırma aynı hash'i üretmek zorundadır (test edilir).
- Tasarım iş parçacığı *gerektirmez* (web hedefi için).

## Motor deposu düzeni

```
wish-engine/
  crates/        yukarıdaki crate'ler
  examples/      küçük örnek projeler (aynı zamanda test verisi)
  evals/         ajan değerlendirme görevleri
  docs/          bu belgeler + kullanıcı kılavuzu
  CLAUDE.md      motoru geliştiren ajan için çalışma kuralları
```
