# Wish Engine

*Dile, olsun.*

Yapay zekâya tamamen entegre ama yapay zekâsız da eksiksiz kullanılabilen, açık kaynak, ücretsiz ve
komisyonsuz bir 3B oyun motoru.

*A free, open-source 3D game engine that works fully by hand, and that an AI assistant can drive
fully through MCP. [English below.](#english)*

## Ne yapabiliyor

- **Editör:**
  - Hiyerarşi, denetçi, sahnede tutamaçlar, varlık tarayıcısı, prefab'lar.
  - Oynat / duraklat / adım ve zaman çizgisi.
  - Tekrar kayıtları ve senaryo testleri.
  - 12 dil.
- **Yapay zekâ kokpiti:**
  - Göster ve söyle; canlı etkinlik akışı.
  - Her değişiklik tek başına geri alınabilir.
  - Her görevden önce denetim noktası.
  - İzin kipleri: yalnızca öner / uygula, ben incelerim / tam özerk.
  - Dışarıdan herhangi bir MCP ajanı bağlanabilir.
- **Oyun sistemleri:**
  - Fizik (Rapier) ve karakter hareketi.
  - Ses; nokta ve spot ışıklar (kümelenmiş forward+); gölgeler.
  - glTF modeller ve iskelet animasyonu.
  - Oyun içi arayüz, parçacıklar, 2B sprite'lar.
- **Oyun kodu:** Rust, korumalı alanda WebAssembly.
  - Oyunu durdurmadan sıcak yükleme.
  - Derleyici pakette gelir; ek kurulum yok.
- **Belirlenimcilik:** aynı sahne ve tohum her makinede bit bit aynı oynar. Testler ve tekrar
  kayıtları buna dayanır.
- **Dışa aktarma:** motor kurulu olmayan makinede çalışan bağımsız oyun klasörü.

## Başlarken

```bash
wen edit
```

Proje olmadan karşılama ekranı açılır:
1. yardımcıyı seç (Claude Code ya da elle);
2. şablon seç;
3. ad ver ve oluştur.

Ayrıntılar: [Kullanıcı kılavuzu](docs/user/tr/kilavuz.md).

## Kaynaktan derleme

Rust 1.100.0 gerekir (`rust-toolchain.toml` ile sabit). Windows'ta Visual Studio Build Tools (C++)
de gerekir.

```bash
cargo build --release -p wish_cli -p wish_player
```

```bash
cargo test --workspace
```

## Belgeler

| Belge | İçerik |
|---|---|
| [docs/user/tr/kilavuz.md](docs/user/tr/kilavuz.md) | Kullanıcı kılavuzu (Türkçe) |
| [docs/user/en/guide.md](docs/user/en/guide.md) | User guide (English) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Katkı kılavuzu: değişmezler, kurallar, doğrulama |
| [docs/VISION.md](docs/VISION.md) | Ne yapıyoruz, neden |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Katmanlar ve crate'ler |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Mimari kararlar |
| [docs/MCP_API.md](docs/MCP_API.md) | Yapay zekâ arayüzü |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Kilometre taşları |

## English

Wish Engine has:
- an editor;
- an AI cockpit;
- game systems: physics, audio, lights and shadows, glTF and animation, in-game interface,
  particles, 2D;
- Rust game code sandboxed as WebAssembly, with hot reload;
- deterministic simulation, so replays and scenario tests are exact;
- standalone game export.

Start with `wen edit` and follow the welcome screen. See the
[user guide](docs/user/en/guide.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## Lisans / License

MIT ya da Apache-2.0, size hangisi uygunsa. Third-party licenses:
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
