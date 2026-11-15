# Wish Engine — Motoru Geliştiren Ajan İçin Çalışma Kuralları

Wish Engine: yapay zekâya tamamen entegre ama yapay zekâsız da eksiksiz kullanılabilen (hibrit), açık kaynak, ücretsiz ve komisyonsuz dağıtılacak, MCP-yerel, deterministik bir 3D oyun motoru. Rust ile yazılır.

## Şu anki durum

- **Tamamlanan:** M0 (tasarım).
- **Süren:** M1 (çekirdek). Tasarım notu: [docs/design/M1-cekirdek.md](docs/design/M1-cekirdek.md) — M1'de kod yazmadan önce oku.
  - Biten: WP1 (çalışma alanı, lint'ler, lisanslar, CI dosyası, git), WP2 (`wish_core`: `Entity`, `Text`, `Color`, matematik türleri, `Diagnostic`, kararlı hash, öneri yardımcıları), WP3 (`wish_schema` + `wish_schema_macros`: şema modeli, `Plain` sözleşmesi, `#[component]`/`#[resource]`/`#[event]`/`#[schema]`, JSON ⇄ bayt, JSON Schema, parmak izi). Toplam 60 test.
  - **Sıradaki: WP4 (`wish_ecs`).** Önerilen model · efor: Opus 5.5 · high. Başlamadan önce tasarım notunun 4. bölümünü ve 12. bölümündeki uygulama notlarını oku. `unsafe` ilk kez burada yazılacak: Miri için nightly araç zinciri kurulması gerekir (indirme; kullanıcıya haber ver).
- **Git:** `main` yalnızca M0 belgelerini taşır; M1 çalışması `m1-core` dalındadır. Uzak depo (remote) yok. Commit kimliği depoya özel proje kimliğidir (kullanıcı kararı); kişisel e-posta kullanma.
- **Ortam (15 Kasım 2026'da doğrulandı):** Rust 1.100.0 (`rust-toolchain.toml` ile sabit) ve Visual Studio 2022 Build Tools 17.14.45 kurulu. Bu oturumun kabuğunda `cargo` PATH'te olmayabilir; her komuttan önce `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"` çalıştır.
- **Proje klasörü OneDrive altında** (`Masaüstü\wish-engine`). Derleme çıktıları git'e girmeyen yerel `.cargo/config.toml` ile `C:/wish-build/target` klasörüne yönlendirildi. Bu dosyayı silme; `target/` klasörünü asla proje klasörünün içinde bırakma.
- **CI henüz çalıştırılmadı:** `.github/workflows/ci.yml` ve `deny.toml` yazıldı ama uzak depo olmadığı için sınanmadı. Depo GitHub'a ilk gönderildiğinde doğrula.

## Önce oku

| Ne zaman | Belge |
|---|---|
| Her oturum başında | Bu dosya + [docs/ROADMAP.md](docs/ROADMAP.md) içindeki etkin kilometre taşı |
| Tasarım kararı gerektiğinde | [docs/DECISIONS.md](docs/DECISIONS.md) |
| Parçaların nasıl birleştiğini görmek için | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| MCP/komut satırı yüzeyine dokunurken | [docs/MCP_API.md](docs/MCP_API.md) |
| "Bu neden var?" sorusunda | [docs/VISION.md](docs/VISION.md) |

## Kullanıcıyla çalışma

- Kullanıcıyla **Türkçe** konuş.
- **Her kilometre taşına (ve belirgin biçimde farklı her iş paketine) başlamadan önce önerilen model ve efor değerini söyle**; yol haritasındaki tabloyu esas al, gerekiyorsa gerekçesiyle güncelle. Kullanıcı bunu açıkça istedi.
- Kullanıcının vizyonu: her şey **sorunsuz** olmalı ve seçimlerde **ileriyi gören, yenilikçi** seçenek tercih edilmeli. İki seçenek arasında kalırsan bunu ölçüt al; ama kapsamı büyüten her öneriyi açıkça "bu 1.0 kapsamını büyütür" diye belirt.
- Kullanıcıya yalnızca gerçekten onun vermesi gereken kararları sor (ad, lisans, para/hesap gerektiren işler). Teknik kararları gerekçesiyle kendin ver ve belgeye yaz.

## Değişmezler (çiğnenemez)

Bir değişiklik bunlardan birini bozuyorsa değişiklik yanlıştır. Gerekçeler [docs/DECISIONS.md](docs/DECISIONS.md) içinde.

1. **Şema önce.** Şeması olmayan tür dünyaya giremez. Her alanın belge yorumu, uygunsa birimi ve aralığı olur.
2. **Determinizm.** Simülasyon kodunda duvar saati, tohumsuz rastgelelik, sırasız koleksiyon üzerinden yineleme, iş parçacığı zamanlamasına bağlı sonuç **yasak**.
3. **Dosyalar doğruluk kaynağıdır.** Kanonik biçim; yükle → kaydet bayt düzeyinde aynı sonucu verir.
4. **Tek kontrol düzlemi.** Yeni yetenek önce `wish_control` içine eklenir; MCP, komut satırı ve arayüz onu çağırır. Yalnızca arayüzde bulunan yetenek olamaz.
5. **Tek tanı formatı.** Her hata `code`, `message`, konum ve **`hint`** taşır. İpucu yazılmamış hata tamamlanmamış sayılır.
6. **Bağımlılık yönü aşağı.** Alt sistemler birbirini tanımaz; ECS üzerinden konuşur. Dış kütüphaneler yalnızca kendi sarmalayıcı crate'lerinden kullanılır.
7. **Oyun kodu güvenilmezdir.** Wasm konuğu motoru çökertememeli, donduramamalı; durum konukta tutulmaz.
8. **Bir özellik; kontrol düzleminden gözlemlenebilir/denetlenebilir olmadan ve başsız testi yazılmadan bitmiş sayılmaz.**
9. **Hibrit eşlik.** Motor, hiçbir yapay zekâ bağlı değilken eksiksiz çalışır. Ajanın yapabildiği her sahne/varlık işlemi arayüzden elle de yapılabilir. Elle ve ajanla yapılan değişiklikler aynı geçmişe, kimin yaptığı bilgisiyle (`actor`) girer.

## Çalışma yöntemi

1. Kod yazmadan önce kısa bir tasarım notu yaz: `docs/design/Mx-konu.md` (sorun, seçenekler, karar, test planı).
2. Küçük adımlarla ilerle: her adım derlenir, test edilir, commit edilir.
3. "Bitti" deme, **göster**: çıkış ölçütünü kanıtlayan komutu çalıştır ve çıktısını raporla. Test başarısızsa bunu çıktısıyla birlikte söyle.
4. Bir ADR'yi değiştirmen gerekiyorsa önce [docs/DECISIONS.md](docs/DECISIONS.md) dosyasını güncelle, sonra kodu.
5. Kilometre taşı bitince [docs/ROADMAP.md](docs/ROADMAP.md) ve bu dosyadaki "Şu anki durum" bölümünü güncelle.
6. Kapsam dışı bir fikir çıkarsa uygulama; M10 listesine not düş.

## Kod uzlaşımları

- Kod, tanımlayıcılar, API adları, tanı mesajları, kod yorumları ve commit iletileri **İngilizce**. Planlama belgeleri Türkçe.
- Crate öneki `wish_`; komut satırı aracı `wen` (`wish` değil: o ad Tcl/Tk'nin komutudur ve çakışır).
- Lisans: MIT + Apache-2.0 (açık kaynak). Her crate'in `Cargo.toml` dosyasında `license = "MIT OR Apache-2.0"`.
- `cargo fmt` ve uyarısız `cargo clippy` zorunlu.
- `unsafe` yalnızca `wish_ecs` ve `wish_script` içinde, bir de `wish_schema`'daki `Plain` sözleşmesinin bildiriminde (ve makronun ürettiği `unsafe impl Plain`'de); her blokta neden güvenli olduğunu açıklayan `// SAFETY:` yorumu; Miri ile test. Diğer her yerde çalışma alanı lint'i `unsafe_code = "deny"`.
- Kütüphane kodunda `unwrap`/`expect`/`panic!` yok (testler hariç); hatalar `Diagnostic` olarak döner. Bunu clippy zorlar (`Cargo.toml` içindeki `[workspace.lints]`).
- Simülasyon kodunda `HashMap`/`HashSet` üzerinde gezinme yasak (clippy `iter_over_hash_type`); sıralı gezinme gerekiyorsa `BTreeMap` ya da `Vec` kullan.
- Kararlı hash gereken her yerde (dünya hash'i, parmak izi, diske yazılan her şey) `wish_core::StableHasher` kullan; standart kitaplığın hash'leyicilerini asla.
- Dünyaya giren türler yalnızca `#[component]`, `#[resource]`, `#[event]`, `#[schema]` ile tanımlanır; `Plain` elle uygulanmaz. Bu makrolar `Clone`, `Copy`, `Debug`, `PartialEq`, `Default` ve `repr`'ı kendileri ekler (bunları ayrıca yazmak derleme hatasıdır).
- Hata döndüren işlevlerde `wish_core::Result<T>` kullan. `Diagnostic` tek işaretçi genişliğindedir; onu büyütecek bir değişiklik yapma (bir test bunu denetler).
- Bütünleşme testi dosyalarının (`tests/*.rs`) başına `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` koy: clippy yalnızca `#[test]` işlevlerini test sayar, ortak yardımcıları saymaz.
- Herkese açık her öge belge yorumu taşır (bunlar şemaya ve MCP belgelerine akar).
- Koordinat sistemi: sağ el, +Y yukarı, kamera −Z'ye bakar. Birimler: metre, saniye, kilogram; API'de radyan.
- Bağımlılık eklemeden önce lisansını denetle (yalnızca MIT/Apache-2.0/BSD benzeri serbest lisanslar).

## Doğrulama komutları

Her commit'ten önce üçü de temiz olmalı (proje kökünde, PowerShell):

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Commit iletisini her zaman dosyadan ver (`git commit -F <dosya>`): PowerShell, `-m` ile verilen iletideki çift tırnakları bozar ve commit yarıda kalır.

Bağımlılık eklediysen lisansını gör: `cargo metadata --format-version 1 | ConvertFrom-Json` çıktısındaki `packages[].license` alanı; izinli liste `deny.toml` içinde.

Henüz yok, ilgili iş paketinde eklenecek: Miri (WP4), determinizm testi (WP8), kıyaslama (WP8).
