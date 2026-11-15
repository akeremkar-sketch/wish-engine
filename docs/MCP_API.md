# Wish Engine — MCP API Taslağı (v0)

Bu, M4'te uygulanacak araç yüzeyinin **taslağıdır**. Kesin JSON şemaları M4'ün ilk oturumunda yazılır ve ajan değerlendirmeleriyle sınanır. İlkeler: [DECISIONS.md](DECISIONS.md) ADR-009, ADR-010.

## Bağlantı

| Taşıma | Kullanım |
|---|---|
| stdio | `wen mcp` — Claude Code / Claude Desktop sunucuyu kendisi başlatır. `wen new` projeye `.mcp.json` yazar, bağlantı otomatiktir. |
| Streamable HTTP | `wen mcp --http` — çalışan editöre ya da oyuna bağlanma; yalnızca yerel makine, oturum belirteciyle. |

Her MCP aracının birebir komut satırı karşılığı vardır (`wen entity query …`, `wen sim step …`), `--json` ile aynı çıktıyı verir.

## Ortak kurallar

- **Yanıt zarfı:** `{ "ok": true, … }` ya da `{ "ok": false, "error": Diagnostic }`.
- **Bütçe:** varsayılan yanıt kısa tutulur. `detail: "summary" | "normal" | "full"` ve `cursor` ile sayfalama.
- **Değiştiren araçlar** `dry_run` kabul eder; fark, tanılar ve yeni `revision` döndürür.
- **Eşzamanlılık:** `base_revision` verilirse ve belge o arada değiştiyse işlem reddedilir, güncel revizyon bildirilir.
- **Kim yaptı (`actor`):** her işlem `human` ya da ajan adını taşır. Arayüz hibrit olduğu için (ADR-018) insan aynı sahneyi elle de düzenler; ajan `edit_history` ile insanın elle yaptığı değişiklikleri görür ve değiştiren araçların yanıtında "son çağrınızdan beri başkasının değiştirdiği varlıklar" özetini alır.
- **Varlık adresleme:** sahne kimliğiyle (`"player"`); oynatma oturumunda çalışma zamanı kimliğiyle de.
- **İfadeler:** sorgu ve doğrulamalarda küçük bir ifade dili (CEL alt kümesi): `Transform.translation.y < 0 && has(RigidBody)`.

## Araçlar

### Proje ve bilgi

| Araç | Amaç | Öne çıkan parametreler |
|---|---|---|
| `project_info` | Proje özeti: sahneler, betik durumu, açık oturumlar, motor sürümü, bekleyen tanılar | — |
| `schema_get` | Bileşen/kaynak/olay şemaları: alanlar, türler, birimler, aralıklar, belge yorumları | `names`, `search`, `detail` |
| `docs_search` | Motor kılavuzunda arama (belgeler ayrıca `wish://docs/…` kaynakları olarak da sunulur) | `query` |
| `check` | Tüm projeyi doğrula: sahneler, başvurular, şemalar, betik derlemesi | `paths` |

### Sahne yazımı (düzenleme belgesi)

| Araç | Amaç | Öne çıkan parametreler |
|---|---|---|
| `scene_get` | Sahne ana hattı: hiyerarşi ağacı, varlık adları, bileşen adları | `scene`, `root`, `depth`, `detail` |
| `entity_query` | Varlık bul: bileşene, ada, ifadeye, uzamsal yakınlığa göre | `with`, `where`, `near`, `limit`, `cursor` |
| `entity_get` | Varlıkların bileşen değerleri | `ids`, `components` |
| `edit_apply` | **Atomik işlem**: `spawn`, `despawn`, `set` (birleştirme), `replace`, `remove`, `reparent`, `rename`, `set_notes`, `set_resource` / `replace_resource` / `remove_resource`, `instantiate`, `set_override` / `replace_override` / `remove_override` işlemlerinin listesi (M2'de `wish_scene::EditOp` olarak uygulandı) | `ops`, `base_revision`, `dry_run` |
| `edit_history` | Geri al / yinele / geçmişi listele | `action`, `steps` |
| `asset_list` | Varlık dosyaları ve içe aktarma durumu | `kind`, `search` |
| `asset_info` | Tek varlık raporu: üçgen sayısı, sınırlar, materyaller, alt nesneler, ölçek uyarıları | `path` |
| `asset_move` | Varlık dosyasını/klasörünü taşı, tüm sahne ve prefab başvurularını aynı işlemde güncelle (`wen mv`) | `from`, `to`, `dry_run` |

### Simülasyon (oynatma oturumu)

| Araç | Amaç | Öne çıkan parametreler |
|---|---|---|
| `sim_start` | Sahneden oturum başlat | `scene`, `seed`, `headless` |
| `sim_step` | İlerle: N tick ya da koşul sağlanana dek; girdi enjekte et | `session`, `frames`, `until`, `input`, `max_frames` |
| `sim_snapshot` | Anı kaydet / listele | `session`, `label` |
| `sim_restore` | Kayıtlı ana dön ya da ondan yeni oturum çatalla | `snapshot`, `fork` |
| `sim_stop` | Oturumu kapat | `session` |
| `replay` | Tekrar kaydını sakla / çalıştır / hash sapmasını bul | `action`, `path` |

Oturum sırasında `entity_query`, `entity_get` ve `edit_apply` `session` parametresiyle canlı dünyaya da uygulanır.

### Gözlem

| Araç | Amaç | Öne çıkan parametreler |
|---|---|---|
| `capture_frame` | Görüntü al (MCP görsel içeriği olarak döner) | `camera`, `size`, `view`: `lit` \| `ids` \| `depth` \| `normals` \| `wireframe` \| `colliders` |
| `describe_view` | Anlamsal görüş: görünen varlıklar, ekran kutuları, görünürlük yüzdesi, uzaklık | `camera`, `limit` |
| `logs_get` | Yapılandırılmış günlükler ve olaylar (ses olayları dahil) | `since`, `severity`, `kinds` |
| `profile_get` | Kare süresi dökümü, sistem başına süre, çizim çağrıları, bellek | `session`, `frames` |

### Doğrulama ve betik

| Araç | Amaç | Öne çıkan parametreler |
|---|---|---|
| `test_run` | Senaryo testlerini koş; başarısızlıkta ilgili tick, beklenen/bulunan değer, görsel fark | `paths`, `update_golden` |
| `script_build` | Oyun kodunu derle; yoğunlaştırılmış tanılar; başarılıysa sıcak yükle | `reload` |

**Toplam: 24 araç.** (`asset_move` M2'de eklendi: birden çok belgeyi ve diski birlikte değiştirdiği için tek sahneye bağlı `edit_apply`'ın parametresi olamaz.) Yeni araç eklemek için gerekçe aranır; önce mevcut bir aracın parametresi olarak çözülüp çözülemeyeceğine bakılır.

## Örnek yanıtlar

`sim_step` — yalnızca ilerlemez, o aralıkta olanları özetler:

```json
{
  "ok": true,
  "session": "s1",
  "tick": 600,
  "stopped_by": "frames",
  "world_hash": "9f2c41d07a3e…",
  "events": { "CollisionStarted": 14, "Despawned": 2 },
  "diagnostics": [
    {
      "severity": "warning",
      "code": "entity_out_of_bounds",
      "message": "Entity 'crate_7' is at y = -312 m; it probably fell out of the world.",
      "hint": "Add a floor collider below it, or add a KillZone to despawn fallen entities."
    }
  ]
}
```

`edit_apply` — hatalı işlem hiçbir şeyi değiştirmez:

```json
{
  "ok": false,
  "error": {
    "severity": "error",
    "code": "value_out_of_range",
    "message": "Mover.speed = 500 is outside the allowed range 0..=50 (m/s).",
    "location": { "op": 2, "path": "/entities/player/components/Mover/speed" },
    "hint": "Use a value within range, or widen the range in the component definition in scripts/src/lib.rs.",
    "docs": "wish://docs/components/Mover"
  }
}
```

## Tipik ajan döngüsü

```
1. project_info                       → neredeyim, ne var?
2. schema_get(search: "light")        → hangi bileşenler, hangi alanlar?
3. edit_apply([spawn zemin, ışık, kamera, oyuncu])
4. (scripts/src/lib.rs dosyasını düzenle) → script_build
5. sim_start → sim_step(frames: 120, input: {move:[0,1]})
6. describe_view + capture_frame      → beklediğim şey ekranda mı?
7. sorun var → sim_restore → düzelt → 5'e dön
8. tests/…test.json yaz → test_run    → kanıt
```

## MCP kaynakları ve istemler

- `wish://docs/guide` — kısa motor kılavuzu (ajanın bağlamına sığacak boyutta)
- `wish://docs/components/{Name}` — bileşen belgeleri (şemadan üretilir)
- `wish://docs/diagnostics/{code}` — her tanı kodunun açıklaması ve çözümü
- `wish://project/status` — canlı proje durumu
- İstemler: "yeni oyun başlat", "hatayı yeniden üret ve düzelt", "performansı incele"

## Tasarımın nasıl sınanacağı

Araç yüzeyi `evals/` altındaki görevlerle ölçülür (ADR-017). Her araç değişikliğinde izlenenler: görev başarı oranı, görev başına araç çağrısı sayısı, görev başına belirteç, ajanın ilk denemede geçersiz çağrı yapma oranı. Sayılar kötüleşiyorsa değişiklik geri alınır.
