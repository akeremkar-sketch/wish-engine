# M4 — Kontrol düzlemi ve MCP: tasarım notu

Durum: yazıldı (15 Kasım 2026). Kod yazmadan önce oku. Bağlam: [ROADMAP](../ROADMAP.md) M4, [DECISIONS](../DECISIONS.md) ADR-009/010/017/018/024, [MCP_API](../MCP_API.md) (araç taslağı), [ARCHITECTURE](../ARCHITECTURE.md) "Dünya ve oturum modeli", "Bir düzenlemenin yolculuğu".

## 1. Hedef ve sınırlar

M4 sonunda Claude (ya da herhangi bir MCP istemcisi), `wen new` ile açılmış boş bir projede **yalnızca MCP araçlarıyla** sahne kurar, çalıştırır, görüntü alır ve sonucu doğrular. Aynı her şey `wen` komut satırından da yapılır ve **aynı JSON'u** verir. İlk 10 değerlendirme görevi orta sınıf modelle (Sonnet 5.5 · medium) ≥ %80 başarır.

**Kapsam dışı (M4'te yok):**
- Oyun kodu ve `script_build` (Wasm, M5). Bileşen türleri M4'te motorun kendi türleri + bir yerleşik örneğin türleridir (§4).
- Fizik, girdi eylemleri: `sim_step`'in `input` parametresi M5'te (eylem haritası yok). `capture_frame`'in `colliders` görünümü M5.
- `test_run` (senaryo testleri, ADR-017 katman 2) ve `replay`: M5 (oyun kodu ve girdi olmadan anlamlı senaryo az). `profile_get`: M6 (render/oyun profili oturduğunda).
- **Streamable HTTP taşıması: M8a'ya ertelendi.** *(Plan değişikliği; kapsamı büyütmez, kaydırır.)* Gerekçe: HTTP'nin amacı "çalışan editöre ya da oyuna bağlanmak"; M4'te bağlanılacak uzun yaşayan bir süreç yok — `wen mcp` stdio sunucusu zaten o süreç. Editör (M8a) geldiğinde HTTP, editörün içinde aynı `wish_control` çekirdeğine açılır. Şimdi yapılırsa test edilecek gerçek bir kullanım olmadan bir HTTP yığını ve oturum belirteci güvenliği eklenmiş olur.

**Değişmezler:** 4 (tek kontrol düzlemi) bu kilometre taşının kendisidir: `wen`'in mevcut proje komutları da (`check`, `fmt`, `mv`, `capture`, `describe`, `shaders`) `wish_control`'e taşınır; komut satırı yalnızca bayrakları argüman nesnesine çevirir. 5 (tek tanı formatı): araç hataları da `Diagnostic`. 9 (hibrit): her işlem `actor` taşır; elle (komut satırı/dosya) ve ajanla yapılanlar aynı geçmişe girer.

## 2. Crate'ler ve bağımlılık yönü

| Crate | Sorumluluk | Dış bağımlılık |
|---|---|---|
| `wish_control` | **Çalışma alanı** (`Workspace`: proje, açık belgeler + geçmiş, oynatma oturumları, renderer), **araç kataloğu** (ad, açıklama, parametre şeması, çıktı), **dağıtıcı** `call(name, args, actor) -> Value`, ifade dili, dünya denetçisi, tanı kataloğu | — (serde_json zaten var) |
| `wish_mcp` | MCP sunucusu: JSON-RPC 2.0 stdio, `initialize`/`tools`/`resources`/`prompts`, `wish://` kaynakları. Yalnızca `wish_control`'ü tanır | — |
| `wish_cli` | İnce istemci: `wen call`, `wen do`, `wen mcp`, `wen new` + kısayol komutları (`capture`, `check`…), hepsi `wish_control::call` | — |

Yön: `wish_control` → `wish_scene`, `wish_asset`, `wish_app`, `wish_render`, `wish_import`, `wish_transform`, `wish_examples`; `wish_mcp` → `wish_control`; `wish_cli` → ikisi. `wish_window` kontrol düzlemine bağlı değil (`wen view` M8a'da editörle birleşir).

**Karar — MCP'yi kendimiz uyguluyoruz (resmî Rust SDK'sı `rmcp` değil).** Gereken yüzey küçük (stdio üzerinde satır başına bir JSON-RPC iletisi; ~10 yöntem). Kendi uygulamamız: (1) eşzamansız çalışma zamanı (tokio) gerektirmez — motor tek iş parçacıklı, deterministik ve GPU kilidiyle çalışıyor; (2) her hata yolunu bizim `Diagnostic`imize eşleriz (SDK'nın hata türlerini çevirmek yerine); (3) bağımlılık ağacı ve lisans yüzeyi küçük kalır; (4) protokol sürümü pazarlığını biz kontrol ederiz. Bedel: protokol değiştikçe güncellemek bizde. Önlem: kayıtlı gerçek oturum dökümleriyle uyum testleri (§9) ve WP6'da Claude Code'la gerçek bağlantı denemesi. Hedef protokol sürümü **2025-06-18** (yapılandırılmış çıktı `structuredContent`, `outputSchema`, araç `title`'ı); istemci daha yeni bir sürüm isterse bizimkiyle yanıtlarız (spesifikasyonun pazarlık kuralı), eskisini isterse destekliyorsak onu kullanırız.

## 3. Çalışma alanı modeli

```
Workspace (bir süreç = bir proje)
├── project: kök klasör, wish.json (ad, türler, varsayılan sahne)
├── registry: motor türleri + proje türleri (M4: yerleşik örnek; M5: Wasm modülü)
├── documents: sahne yolu → { SceneDocument, geçmiş (actor'lı), disk parmak izi }
├── sessions: "s1", "s2"… → { App, kaynak sahne + revizyon, tohum, anlık görüntüler }
├── renderer: tembel (ilk capture/describe çağrısında GPU açılır; GPU yoksa yalnızca o araçlar hata verir)
└── actors: actor → son gördüğü revizyon (başkasının değişikliklerini bildirmek için)
```

- **Dosyalar doğruluk kaynağı (değişmez 3).** Her değiştiren çağrı belgeyi kanonik biçimde **hemen** diske yazar (atomik: geçici dosya + yeniden adlandırma). Her çağrının başında çalışma alanı açık belgelerin disk parmak izine bakar (M3'teki `fingerprint_dir` yaklaşımı, belge başına): dışarıdan değişmişse yeniden yükler ve bunu geçmişe `Actor::File` olarak yazar. Geçersizse son geçerli belge kalır ve çağrının yanıtına tanı eklenir.
- **Belgeler tembel açılır:** bir araç bir sahneye ilk dokunduğunda yüklenir; `scene` parametresi verilmezse `wish.json`'daki varsayılan sahne.
- **Oturumlar** belgenin o anki revizyonundan kopyalanır (`sim_start`), diske yazmaz, birbirinden bağımsızdır; `sim_restore(fork: true)` yeni oturum açar. Oturum kimlikleri sıralı (`s1`, `s2`…): deterministik ve ajanın okuması kolay.
- **Tek iş parçacığı.** MCP sunucusu çağrıları sırayla işler; eşzamanlılık sorunu yok. Uzun `sim_step` için `max_frames` üst sınırı (varsayılan 10 000, en çok 1 000 000) ve iptal bildirimi (`notifications/cancelled`) tick sınırında denetlenir.
- **Kim yaptı:** MCP'de `actor`, istemcinin `initialize`'da verdiği `clientInfo.name`'den (`Agent("claude-code")`); komut satırında `Human` (değiştirilebilir: `--actor`). Değiştiren her yanıtta `changed_by_others`: bu aktörün son çağrısından beri başka aktörlerin dokunduğu varlıklar (en çok 20, fazlası sayı olarak). Ajan, insanın elle taşıdığı nesneyi böyle görür (ADR-018 kural 4).

## 4. Proje dosyası ve türler

`wen new <klasör> [--name <ad>]` şunları yazar:

```
wish.json              { "format": 1, "name": "...", "engine": "0.1.0", "scene": "scenes/main.scene.json", "types": { "example": "stress" } }
scenes/main.scene.json kamera + güneş + zemin (boş ama çizilebilir başlangıç)
assets/                boş
.mcp.json              { "mcpServers": { "wish": { "command": "wen", "args": ["mcp"] } } }
CLAUDE.md              projeye özel kısa kılavuz (araç döngüsü, kurallar; wish://docs/guide'a yönlendirir)
.gitignore
```

- `wen` PATH'te değilse `.mcp.json`'a `wen`'in tam yolu yazılır (`std::env::current_exe`). Proje taşınabilirliği için bir sonraki `wen new`/`wen mcp` bunu tazeleyebilir; M10'a not.
- **Türler:** motor türleri her zaman (`wish_cli::types`'taki liste `wish_control`'e taşınır). `types.example` M4'ün geçici yoludur (bugünkü `--example` bayrağının proje dosyasına taşınmış hali); M5'te `types.module: "scripts"` gelir. `wen`'in `--example` bayrağı, `wish.json` yoksa geriye dönük çalışmaya devam eder.
- `wish.json` da kanonik JSON'dur ve `wen check` onu da doğrular.

## 5. Araç kataloğu (M4)

Taslaktaki 24 araçtan M4'te **20**'si uygulanır; `script_build`, `test_run`, `replay` M5'te, `profile_get` M6'da gelir.

| Grup | Araçlar |
|---|---|
| Proje | `project_info`, `schema_get`, `docs_search`, `check` |
| Sahne | `scene_get`, `entity_query`, `entity_get`, `edit_apply`, `edit_history` |
| Varlık | `asset_list`, `asset_info`, `asset_move` |
| Simülasyon | `sim_start`, `sim_step`, `sim_snapshot`, `sim_restore`, `sim_stop` |
| Gözlem | `capture_frame`, `describe_view`, `logs_get` |

(20 araç. M5'te `script_build`, `test_run`, `replay`; M6'da `profile_get` → 24.)

**Ortak sözleşme** (taslaktaki kurallar, kesinleştirilmiş):
- Yanıt zarfı `{ "ok": true, ... }` / `{ "ok": false, "error": Diagnostic }`. Uyarılar `diagnostics: [...]` (yalnızca varsa).
- **Parametre doğrulama tek yerde:** her aracın parametreleri bir tabloyla tanımlanır (ad, tür, zorunlu mu, varsayılan, aralık, belge). Aynı tablo (1) MCP `inputSchema`'sını (JSON Schema), (2) komut satırı yardımını, (3) ayrıştırıcıyı üretir. Bilinmeyen parametre → `unknown_parameter` + "did you mean" + `candidates`; yanlış tür → `invalid_parameter` + beklenen tür ve örnek. (serde'nin genel hataları yerine; ajanın ilk denemede düzeltebilmesi için.)
- `detail: "summary" | "normal" | "full"` (varsayılan `summary`) ve `limit`/`cursor` liste döndüren araçlarda. İmleç opak dizgi, deterministik.
- Değiştirenler (`edit_apply`, `edit_history`, `asset_move`): `dry_run`, `base_revision`; yanıtta `revision`, `diff` (varlık başına eklenen/silinen/değişen bileşen alanları, JSON Pointer'larla), `changed_by_others`.
- Varlık adresleme: sahne kimliği (`"player"`); oturumlarda da sahne kimliği (oturumda doğan varlıklar için `"#<sayı>"` çalışma zamanı kimliği).
- **İfade dili** (`entity_query.where`, `sim_step.until`): küçük, deterministik bir alt küme — alan yolları (`Transform.translation.y`), sayı/dizgi/mantık sabitleri, `+ - * /`, karşılaştırmalar, `&& || !`, `has(Bileşen)`, `abs()`, `len()`, `distance(a, b)` (vektörler). Ayrıştırma hatası konum + ipucu verir. CEL'in yalnızca bu kesiti; tam CEL bağımlılığı yok.
- **Görseller:** `capture_frame` MCP görsel içeriği (PNG, base64) döner; varsayılan 512×288 (belirteç bütçesi), en çok 2048 kenar. `out` verilirse ayrıca dosyaya yazar. JSON zarfında `image: { mime, width, height, bytes, data? }` — komut satırında `data` yalnızca `--out` yoksa yazılır.

**Önemli araç kararları:**
- `scene_get`: ağaç (sahne kimliği, ad, bileşen adları, çocuklar); `detail: full` bileşen değerlerini de verir. Büyük sahnelerde `root` + `depth`.
- `entity_query`: `with` (bileşen adları), `where` (ifade), `name` (glob: `crate_*`), `near` ({ point, radius } — `Transform.translation`'a göre), `limit` (varsayılan 50), `cursor`. Sonuç yalnızca kimlikler + istenirse `components`.
- `edit_apply`: M2'nin `EditOp` listesi olduğu gibi (`wish_scene` zaten atomik). Hata `location.op` (işlem sırası) + JSON Pointer.
- `sim_step`: `frames` ya da `until` (ifade) + `max_frames`; yanıt: `tick`, `stopped_by` (`frames`|`until`|`max_frames`|`cancelled`), `world_hash`, olay sayıları, dünya denetçisi uyarıları.
- **Dünya denetçisi** (M4 kuralları): sonlu olmayan dönüşüm (`transform_not_finite`), dünyadan düşen varlık (`entity_out_of_bounds`, varsayılan |y| > 1000 m), sıfır ölçek (`degenerate_transform`), kamerasız sahne yakalaması. Kurallar tabloda; M5'te fizik kuralları eklenir.

## 6. Komut satırı eşliği

- `wen call <araç> ['{json args}' | --<param> <değer>…] [--actor <ad>]` — tek çağrı, JSON çıktısı **MCP'deki `structuredContent` ile bayt bayt aynı** (kanonik JSON).
- `wen do [dosya|-]` — JSON dizisi ya da JSONL çağrı listesi, **tek süreçte sırayla**; her sonuç bir satır. Oturumlar ve geri al yığını yalnızca süreç içinde yaşar (ARCHITECTURE: oturum diske yazmaz), bu yüzden durumlu akışların (sim_start → sim_step → capture) komut satırı karşılığı `wen do`dur.
- Kısayollar (`wen capture`, `wen check`, `wen mv`…) korunur; yalnızca bayrakları argümana çevirip `call` yapar.
- **Otomatik eşlik testi:** her araç için örnek çağrı listesi; aynı liste (a) süreç içinde `wish_control`'e, (b) `wen do` sürecine, (c) `wen mcp` sürecine JSON-RPC ile verilir; üç çıktı karşılaştırılır (görseller dahil — aynı yazılım bağdaştırıcısında aynı baytlar). Yeni araç eklenip örneği yazılmazsa test başarısız olur (katalog ↔ örnek listesi tam eşleşmeli).

## 7. MCP sunucusu (`wen mcp`)

- stdio: her satır bir JSON-RPC iletisi (MCP stdio taşıması). stdout yalnızca protokol; günlükler stderr'e.
- Yöntemler: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`, `notifications/cancelled`. Bilinmeyen yöntem → JSON-RPC `-32601`; araç hatası protokol hatası **değildir**: `tools/call` sonucu `isError: true` + zarf (MCP kuralı — ajan hatayı görür ve düzeltir).
- `tools/call` sonucu: `content` (kısa metin özeti + varsa görsel) ve `structuredContent` (tam zarf), `outputSchema` araç kataloğundan.
- **Kaynaklar:** `wish://docs/guide` (el yazımı, ajan bağlamına sığacak ~2–3 bin belirteç), `wish://docs/components/{Name}` (şemadan üretilir), `wish://docs/diagnostics/{code}` (tanı kataloğu), `wish://project/status` (canlı özet). Yanıtlardaki `docs` alanı bunlara işaret eder.
- **Tanı kataloğu:** her tanı kodu için açıklama + tipik düzeltme, `wish_control`'de tek tablo. **Test:** kaynak kodda geçen her `Diagnostic::error/warning("kod", …)` kataloğda olmalı (depo taranır); eksikse test adını söyler. Böylece `docs` bağlantısı hiçbir zaman boşa çıkmaz.
- **İstemler:** "yeni sahne kur", "bir sorunu yeniden üret ve düzelt" (MCP_API'deki "performansı incele" M6'da).

## 8. Değerlendirmeler (`evals/`)

- Her görev bir klasör: `task.md` (ajana verilen istem, kullanıcı dili), `setup` (başlangıç projesi: genelde `wen new` çıktısı), `check.json` (`wen do` ile çalıştırılan doğrulama çağrıları + beklenen sonuçlar: ör. "`entity_query where Transform.translation.y > 2` en az 3 varlık", "`describe_view`'da `player` görünür"). Doğrulama motorun kendi araçlarıyla, insan gözü yok.
- Çalıştırıcı: `evals/run` — her görev için geçici proje kopyası, `claude -p "<task>" --mcp-config <proje>/.mcp.json --model <model> --allowedTools mcp__wish__*` (yalnızca MCP araçları; dosya düzenleme yok → "yalnızca MCP ile" ölçütü yapısal olarak sağlanır), sonra `check.json`. Kayıt: `evals/results/<tarih>-<model>.json` (başarı, araç çağrısı sayısı, belirteç, ilk geçersiz çağrı oranı).
- İlk 10 görev (zorluk artan): boş sahneye küp koy · üç platformlu bölüm · ışığı gün batımına çevir · kamerayı nesneye bakacak yerleştir · prefab'tan beş örnek diz · sahnedeki hatalı değeri bul ve düzelt · simülasyonu 120 tick ilerlet ve düşeni bul · görünmeyen nesneyi görünür yap (describe_view) · modeli (glTF) içe aktar ve yerleştir · insanın elle yaptığı değişikliği fark edip ona göre devam et.
- **Kullanıcıya ait karar:** değerlendirmeleri koşmak Claude kullanımı harcar (abonelik/API). WP7'de koşmadan önce kullanıcıdan onay istenir; kaç tekrar (öneri: görev başına 3, Sonnet 5.5 · medium) ve tahmini maliyet söylenir.

## 9. Test planı (çıkış ölçütleriyle)

| Çıkış ölçütü | Kanıt |
|---|---|
| Boş projeden yalnızca MCP ile sahne kur → çalıştır → görüntü al → doğrula | (a) betikli uçtan uca test: `wen new` + `wen mcp` sürecine JSON-RPC ile 8 adımlı tipik döngü (CI'da, yazılım bağdaştırıcısı); (b) gerçek Claude Code ile `evals/` görevleri (yerelde, onaylı) |
| Her MCP aracının komut satırı karşılığı aynı JSON | §6 otomatik eşlik testi (katalogdaki her araç için) |
| İlk 10 görev Sonnet 5.5 · medium ≥ %80 | `evals/results/` kaydı (görev başına 3 deneme) |
| Geçersiz her çağrı ipuçlu yapılandırılmış hata | Bulanıklaştırma testi: her araç için katalogdan türetilmiş bozuk argümanlar (eksik zorunlu, yanlış tür, bilinmeyen parametre, aralık dışı, olmayan sahne/varlık/oturum) → hepsi `ok:false`, `hint` boş değil, `code` katalogda; süreç asla çökmez |
| (ek) MCP protokol uyumu | Kayıtlı JSON-RPC dökümleri (initialize → tools/list → call → hata → kaynak okuma) altın dosyalar; Claude Code ile elle bağlantı denemesi (WP6) |
| (ek) Hibrit geçmiş | Ajan değişikliği + dışarıdan dosya düzenlemesi + ajan değişikliği → `edit_history` sırayı ve aktörleri doğru gösterir; ajanın yanıtında `changed_by_others` insanın varlığını içerir |

## 10. İş paketleri ve önerilen model

| # | İş paketi | Model · efor |
|---|---|---|
| WP1 | `wish_control` iskeleti: `Workspace`, `wish.json`, tür kaydı taşıma, parametre tablosu → şema/ayrıştırıcı, dağıtıcı, zarf; `project_info`, `schema_get`, `check`; `wen call` / `wen do`; eşlik test düzeneği | Opus 5.5 · high |
| WP2 | Sahne araçları: `scene_get`, `entity_query` (+ ifade dili), `entity_get`, `edit_apply` (`dry_run`, `base_revision`, `diff`, `changed_by_others`), `edit_history`; disk uzlaştırma (`Actor::File`) | Opus 5.5 · high |
| WP3 | Varlık araçları (`asset_list`, `asset_info`, `asset_move`), `wen new` (iskelet, `.mcp.json`, proje `CLAUDE.md`), eski `wen` komutlarının `wish_control`'e taşınması | Sonnet 5.5 · medium |
| WP4 | Oturumlar: `sim_start/step/snapshot/restore/stop`, `until`, iptal, `logs_get`, dünya denetçisi | Sonnet 5.5 · high |
| WP5 | Gözlem: `capture_frame` (görsel içerik), `describe_view`; belgede ve oturumda | Sonnet 5.5 · medium |
| WP6 | `wish_mcp` + `wen mcp`: JSON-RPC stdio, araçlar/kaynaklar/istemler, tanı kataloğu + tarama testi, `docs_search`, `wish://docs/guide`; protokol dökümü testleri; Claude Code ile gerçek bağlantı | Opus 5.5 · high |
| WP7 | `evals/`: 10 görev, çalıştırıcı, denetleyici; **kullanıcı onayıyla** koşum ve kayıt; araç yüzeyini sonuçlara göre düzelt | Opus 5.5 · high (düzenek), değerlendirilen: Sonnet 5.5 · medium |
| WP8 | Bulanıklaştırma testi, uçtan uca betikli test, bağımsız kapanış incelemesi, M4 sonucu, `main`'e birleştirme | Opus 5.5 · high |

## 11. Riskler

- **Araç yüzeyi tahmini yanlış olabilir.** Önlem: WP7 sonuçlarına göre ad/parametre değişikliği serbest (henüz dış kullanıcı yok); her değişiklik eval sayılarıyla gerekçelendirilir (ADR-010).
- **MCP spesifikasyonu değişiyor.** Önlem: sürüm pazarlığı + döküm testleri; protokol katmanı `wish_mcp`'de yalıtık.
- **Belirteç bütçesi:** büyük sahnede `scene_get` patlayabilir. Önlem: varsayılan `summary`, `limit`, yanıt boyutu üst sınırı (aşılırsa kesilir + `truncated: true` + nasıl daraltılacağı ipucu).
- **GPU'suz makine:** `capture_frame`/`describe_view` dışındaki her şey GPU'suz çalışmalı; renderer tembel açılır, `--software` yerine `wish.json`/ortam değişkeniyle yazılım bağdaştırıcısı seçilebilir.
- **Dışarıdan düzenleme yarışı:** dosya tam yazılmadan okunursa (M3'te görülen) — yükleme hatasında 100 ms sonra bir kez yeniden dene, sonra son geçerli belgeyi koru.

## 12. Kullanıcıya ait kararlar / onaylar

1. WP7: değerlendirme koşumları Claude kullanımı harcar — koşmadan önce onay (deneme sayısı ve tahmini maliyetle).
2. HTTP taşımasının M8a'ya kaydırılması teknik karardır (gerekçe §1); kullanıcıya bilgi olarak bildirilir.

## 13. Açık sorular

1. ~~`euler_deg`~~ — WP2'de kapandı (§14). Eski metin: `edit_apply`'da `Transform.rotation` için `euler_deg` kısa yazımı (M3'ten devir): ajan için kuaterniyon yazmak hata kaynağı. WP2'de karar: **evet**, ama yalnızca giriş kolaylığı — belgeye kuaterniyon yazılır (dosya kanonik kalır), yanıtta ikisi de gösterilir.
2. `entity_get` sonuçlarında türetilmiş değerler (`GlobalTransform`) gösterilsin mi? Öneri: oturumlarda evet (salt okunur işaretli), belgede hayır.

## 14. Uygulama notları

### WP1 — `wish_control` iskeleti, `wen call` / `wen do`

- **Crate:** `wish_control` (`param`, `project`, `tool`, `tools/`, `types`, `workspace`). Motor türlerinin listesi `wish_cli::types`'tan buraya taşındı; `wish_cli::types` artık yalnızca eski `--example` komutları için ince bir uyarlayıcı.
- **Parametre tablosu (`Param`, `Kind`):** her aracın parametreleri `const` bir dizi; aynı tablodan `input_schema` (MCP JSON Schema: tür, aralık, `enum`, `default`, `description`, `additionalProperties: false`), `parse` (ayrıştırıcı + varsayılanlar) ve `from_flags` (komut satırı `--ad değer`; liste için virgül ya da JSON, tek başına yazılan bayrak `true`) üretilir. Hata kodları: `invalid_arguments` (nesne değil), `unknown_parameter` (+ "did you mean", `candidates`), `missing_parameter`, `invalid_parameter` (ne beklendiği + örnek yazım + parametrenin belgesi); hepsi `location.path` = `/<parametre>`.
- **Zarf:** `Workspace::call(tool, args, actor) -> Value` hiç başarısız olmaz: araç içindeki bir panik `internal_error` yanıtına döner, süreç ayakta kalır. Yanıt anahtarları sıralı (serde_json `Map` = `BTreeMap`) → aynı çağrı her yerde aynı baytlar.
- **`wish.json`:** `format`, `name`, `scene`, `engine`, `types.example`; konumlu `invalid_project_file` hataları (bilinmeyen ayar → öneri). Bozuk ya da eksik `wish.json` çalışma alanını açmayı engellemez: araçlar motor türleriyle çalışır, `project_info` ve `check` sorunu raporlar (eksikse `no_project_file` uyarısı "`wen new`" ipucuyla).
- **Araçlar:** `project_info` (ad, varsayılan sahne, sahne/prefab listesi, varlık sayısı, `check`'in hata/uyarı sayıları, tür sayıları), `schema_get` (`names` / `search` (tüm sözcükler ad, alan adı ve belgelerde aranır) / `detail`: summary = ilk cümle + alan adları, normal = alanların türü/birimi/aralığı/varsayılanı/belgesi, full = JSON Schema; yönetilen türler `read_only`), `check` (`passed`, dosya/hata/uyarı sayıları, tanılar; `paths` yanlışsa `file_not_found` + en yakın dosya). Araç adı kuralı: `snake_case`, `ad_eylem` ya da tek fiil (`check`, taslaktaki gibi).
- **`wen call <araç> ['{json}' | --param değer …] [--root] [--example] [--actor]`** ve **`wen do [dosya | -]`** (JSON dizisi ya da satır başına bir çağrı; her yanıt bir satır; bozuk liste satır numarasıyla). Çıkış kodu `ok`'tan.
- **Eşlik testi (`wish_cli/tests/parity.rs`):** her araç için örnek çağrılar (bilerek hatalı olanlar dahil) süreç içinde, `wen call` ile ve `wen do` ile koşulur; üç çıktı **bayt bayt** aynı. Katalogdaki bir araç örneksizse test başarısız olur. MCP ayağı WP6'da eklenecek.
- **Test:** `wish_control` birim (parametre ve proje dosyası) + `tests/catalog.rs` (katalog kuralları: belgeler, varsayılanlar kendi denetiminden geçer, değiştiren araçlar `dry_run` taşır; bozuk/eksik `wish.json`; `schema_get` arama). Toplam 316 test yeşil.

### WP2 — sahne araçları, ifade dili, dosyayla uzlaştırma

- **Araçlar:** `scene_get` (ağaç; `root`, `depth`, `detail`; 500 varlıkta kesilir + `truncated` ve daraltma ipucu), `entity_query` (`with`, `where`, `name` glob, `near` {point, radius} dünya konumuna göre — en yakın önce, `components` değerleri, `limit`, `cursor` = `after:<id>`), `entity_get` (etkin değerler: prefab parçaları açılmış, varsayılanlar dolu; `SceneId`/`Hierarchy`/`GlobalTransform` istenmedikçe gizli; prefab'tan gelen parçalar `from_prefab: true`), `edit_apply` (`ops`, `base_revision`, `dry_run`, `create`), `edit_history` (`list` / `undo` / `redo`, `steps`, `dry_run`). Değiştirenler ve `scene_get` `changed_by_others` döndürür: bu aktörün son bakışından beri başka aktörlerin (kişi, dosya, başka ajan) dokunduğu varlıklar ve kim oldukları.
- **İşlem biçimi (`ops.rs`):** `{ "op": "spawn", "id": ..., ... }`; M2'nin 15 `EditOp`'unun hepsi. Hatalar `invalid_op` + `location.op` + `/ops/<n>/<anahtar>` + "did you mean". **`euler_deg` kısa yazımı (açık soru 1 kapandı):** `Transform` değerlerinde `[x, y, z]` derece → `rotation` kuaterniyonu (önce Y sapma, sonra X yunuslama, sonra Z yuvarlanma); dosyaya yalnızca kuaterniyon yazılır (1e-7'ye yuvarlanmış).
- **İfade dili (`expr.rs`):** yollar (`Transform.translation.y`, `[i]`, vektörlerde `.x .y .z .w`, `(a + b).x`), sabitler, `+ - * /` (vektörlerde öğe öğe), karşılaştırmalar (zincirlenmez), `&& || !` (kısa devre), `has`, `abs`, `len`, `length`, `distance`, `min`, `max`, `matches` (glob). Bileşen olmayan varlıkta yol `null`; `null` ve tür uyuşmazlığıyla her karşılaştırma yanlış, sıfıra bölme `null` — sorgu yarıda asla kırılmaz. Derleme sırasında bileşen ve alan adları şemaya karşı denetlenir (`expr_unknown_name` + öneri), sözdizimi hatası sütunla (`expr_syntax`); iç içe derinlik ve uzunluk sınırlı.
- **Belge deposu (`documents.rs`):** sahne ilk dokunuşta açılır; her değişiklik hemen **kanonik ve atomik** (geçici dosya + yeniden adlandırma) yazılır. Her çağrıda dosya yeniden okunur; dışarıdan değişmişse fark **`Actor::File` işlemine** çevrilip uygulanır (ekle → yeniden ebeveynle → kaldır → bileşen/override/not/kaynak değişiklikleri) ve sonucun dosyayla aynı olduğu doğrulanır; aynı değilse dosya olduğu gibi alınır ve `history_reset` bilgisi verilir. Böylece kişinin metin düzenleyicideki değişikliği aynı geçmişe girer ve **geri alınabilir** (testte: ajan → dosya → ajan; iki adım geri al kişinin değişikliğini de geri alır). Dosya geçersiz hale gelirse son geçerli belge kalır, okuma araçları uyarı verir (`external_change_rejected`), değiştirme araçları `scene_file_invalid` ile reddedilir — **kişinin yarım düzenlemesinin üzerine asla yazılmaz**; yarım yazılmış dosya için 100 ms sonra bir kez daha okunur. Prefab dosyaları değişince kütüphane yenilenir.
- **Görünüm:** `entity_query`/`entity_get` belgeyi bir dünyaya örnekleyip okur (revizyon başına önbellek), böylece oturumlarla (WP4) aynı okuma yolu kullanılacak. **Açık soru 2 kapandı:** belgede `GlobalTransform` yalnızca istenirse gösterilir (`near` dünya konumu için onu kullanır).
- **Revizyonlar süreç içinde anlamlıdır:** her süreç belgeyi revizyon 0'dan açar; `base_revision`, geri al ve `changed_by_others` bir oturum (MCP sunucusu, `wen do`) boyunca geçerlidir. Dosya her zaman ortak doğrudur.
- **Eşlik testi** durumlu hale geldi: örnek listesi tek oturum olarak hem süreç içinde hem `wen do` ile (ayrı proje kopyalarında) koşulur, her yanıt ve sonuçtaki sahne dosyası bayt bayt aynı; değiştirmeyen her çağrı ayrıca `wen call` ile taze çalışma alanına karşı karşılaştırılır.
- **Test:** `expr` (3), `ops` (3), `documents` (2) birim; `tests/scene_tools.rs` (7): ajan sahne kurar (dosya kanonik, `check` geçer), sorgular (bileşen, koşul, ad, uzaklık + sıralama, sayfalama, hatalar), etkin değerler ve en yakın kimlik önerisi, kuru çalıştırma/başarısız işlem/eski revizyon hiçbir şeyi değiştirmez, kişi + ajan tek geçmiş ve geri al, bozuk elle düzenlemenin üzerine yazılmaması, yeni sahne + prefab örneği. Toplam 331 test.

### WP3 — varlık araçları, `wen new`, eski komutların taşınması

- **Araçlar:** `asset_list` (`kind`: model/image/other, `search`, `limit`; `wish.json`/`CLAUDE.md` ve `.meta.json` listelenmez), `asset_info` (glTF için üçgen/köşe sayısı, çizilen parça ve üçgen sayısı, metre cinsinden boyut ve sınırlar, malzeme/doku sayısı, içe aktarma uyarıları; olağandışı ölçek `model_scale_unusual` uyarısı: >100 m ya da <1 cm; `.meta.json` içe aktarma ayarları; varlığı kullanan sahne/prefab listesi `used_by`; yanlış yol `asset_not_found` + en yakın yol), `asset_move` (`wish_asset::plan_move/apply_move`; `dry_run`). Taşımadan sonra açık sahneler dosyayla uzlaştırma yoluyla (WP2) güncellenir: taşıma sahnenin geçmişine `file` işlemi olarak girer ve geri alınabilir.
- **`wen new <klasör> [--name]` (`wish_control::scaffold`):** `wish.json`, başlangıç sahnesi (kamera, güneş, zemin — `check` sıfır uyarıyla geçer), `assets/`, `.mcp.json` (`wen`'in **tam yolu**: PATH'te olmasa da istemci başlatabilir), proje `CLAUDE.md`'si (ajan döngüsü ve kurallar), `.gitignore`. Var olan `wish.json` olan klasörü reddeder (`project_exists`), ad doğrulanır. Yetenek kontrol düzleminde, `wen` yalnızca klasör adı ve MCP komut yolunu seçer.
- **Taşınan komutlar:** `wen check` ve `wen mv` artık `check` / `asset_move` araçlarını çağırır; `--json` çıktısı aracın kendi yanıtıdır (MCP'deki ile aynı). Sonuç olarak `wen check --json` artık `passed` alanı taşır (`ok` "araç çalıştı" demektir). `wen run`, `bench`, `capture`, `describe`, `shaders`, `view`, `watch` WP4/WP5/M8a'da, **`wen fmt` ve `wen migrate`** ise bilinçli olarak komut satırında kalır: motor zaten her yazmada kanonik yazıyor, `fmt` yalnızca elle düzenlenen dosyaları düzeltir; `migrate` dosya sürümü yükseltir ve M2'de tek sürüm var. Bunlar bir araç olarak kataloğa girmeden önce ajan değerlendirmelerinde ihtiyaç doğarsa eklenir.
- **Düzeltme (asset yürüyüşü):** geçici dosyalar (`.part`, `.tmp`, `.swp`, `.wish-tmp`, `~`, `.bak`, `#…#`) artık varlık sayılmaz ve `wen watch`/dosya izlemeyi tetiklemez. Atomik yazan editörler ve motorun kendi yazımları geçici dosya bıraktığı için bu bir gözlenen yarış (watch testi üç yerine bir "reloaded" bekliyordu) idi.
- **Test:** `wish_control/tests/asset_tools.rs` (yeni proje temiz geçer ve `.mcp.json` doğru; liste/bilgi/taşıma + referans güncelleme + geçmişte `file`), parity örnekleri (varlık araçları dahil), `wen new` uçtan uca (+ var olan klasör reddi, tam yol). Toplam 334 test.

### WP4 — oturumlar, günlük, dünya denetçisi

- **Araçlar (6):** `sim_start` (`scene`, `seed`; oturum kimliği `s1`, `s2`…, en çok 8 açık), `sim_step` (`frames` ya da `until`, `max_frames`; `session` tek oturum açıksa atlanabilir), `sim_snapshot` (`save` / `list`, `label`; etiket yoksa `snapN`, üzerine yazılmaz), `sim_restore` (`snapshot`, `fork`), `sim_stop`, `logs_get` (`session`, `since`, `severity`, `kinds`, `limit`; `last_seq` ile artımlı okuma). `entity_query` ve `entity_get` `session` alır: canlı dünyayı okur (`tick` döner, `revision` yerine); oturumda doğan varlıklar `#<kimlik>` adını alır, sahne varlıkları sahne kimliğini korur. **`edit_apply`'ın oturumda canlı dünyaya uygulanması M5'e kaldı** (canlı dünyaya `EditOp` çevirisi, oyun sistemleriyle etkileşim ister); oturumda bulunan bir şeyi kalıcı kılmak için aynı değişiklik sahnede `edit_apply` ile yapılır (araç açıklaması bunu söyler).
- **Oturum kurulumu:** proje türleri (+ `wish.json`'daki örneğin türleri ve **sistemleri**: `wish_examples::add_systems`; bounce'ın kaynakları yoksa varsayılanla eklenir, sahnenin değeri öncelikli) → sahne `instantiate` → `transform_propagation` sistemi (tüm diğer sistemlerden sonra; `GlobalTransform` her tick güncel, programın parmak izine ve anlık görüntülere dahil). Oturum diske **hiç yazmaz**. Sahne dosyası oturum sırasında değişirse oturum etkilenmez (kendi kopyası).
- **Determinizm kanıtı (`tests/sessions.rs`):** aynı sahne+tohum → aynı `world_hash`, başka tohum → farklı; anlık görüntüye dönüp yeniden koşmak bayt-aynı hash; **çatal** (`fork`) anlık görüntüden yeni oturumda, orijinalin sürdüğü yerden aynı hash'e varır. Eşlik testi de aynı diziyi süreç içinde ve `wen do` ile (ayrı süreçler) koşar: hash'ler süreçler arasında da eşit.
- **`until`:** koşul ifade dilinde; dünyanın kaynakları için (`Stats.bounces >= 10`, kaynaklar bileşen gibi adlandırılır; `expr` artık kaynak adlarını/alanlarını da denetler) ya da **herhangi bir varlık** için doğru olduğunda tick'ten hemen sonra durur (`stopped_by: until`, `matched`: ilk 10 ad). Hiç doğru olmazsa `max_frames`'te durur + `until_not_reached` uyarısı. Koşul hatası hiçbir tick koşmadan açıklanır (`/until`, sütun). Her tick'te tüm varlıkları değerlendirmek varlık sayısıyla doğrusal artar; büyük dünyalarda `max_frames` küçük tutulmalı (M6'da ölçüp gerekirse örnekleme/indeks).
- **Olaylar:** `wish_ecs::World::events_sent(ad)`: tür başına toplam gönderilen sayısı (tampondan atılanlar dahil); `sim_step` iki okumanın farkını `events: { Bounced: 17 }` olarak verir.
- **Dünya denetçisi (adım sonunda, tekrar bildirmez):** `transform_not_finite` (hata), `entity_out_of_bounds` (|y| > 1000 m, uyarı); sistem hataları (`system_failed`, sistem kapatılır, dünya sürer) `diagnostics`'e girer. Hepsi günlüğe (`kind`: session / system / inspector; en çok 1000 giriş).
- **Test:** `tests/sessions.rs` (6): determinizm ve tohum, olay sayımı + canlı sorgu + kaynakla koşul, `until` (21. tick, `max_frames`, hata), anlık görüntü / geri yükleme / çatal / belirsiz oturum / yanlış kimlik önerisi, denetçi + günlük (`since`, `severity`, `kinds`), sınırlar. Eşlik: 14 yeni örnek çağrı. Toplam 340 test; `wish_ecs` Miri temiz.
