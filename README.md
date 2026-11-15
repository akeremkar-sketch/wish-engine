# Wish Engine

*Dile, olsun.*

Yapay zekâya tamamen entegre ama yapay zekâsız da eksiksiz kullanılabilen, açık kaynak, ücretsiz ve komisyonsuz bir 3D oyun motoru.

**Durum:** geliştirmenin çok başında; henüz çalıştırılabilir bir motor yok. M0 (tasarım) tamamlandı, M1 (çekirdek) sürüyor: `wish_core`, `wish_schema` ve `wish_ecs`'in temeli hazır.

## Derleme

Rust 1.100.0 gerekir (`rust-toolchain.toml` ile sabit; `rustup` kendiliğinden kurar). Windows'ta ayrıca Visual Studio Build Tools (C++ iş yükü) gerekir.

```bash
cargo test --workspace
```

## Belgeler

| Belge | İçerik |
|---|---|
| [docs/VISION.md](docs/VISION.md) | Ne yapıyoruz, neden, kapsam, başarı ölçütleri |
| [docs/DECISIONS.md](docs/DECISIONS.md) | 24 mimari karar ve gerekçeleri |
| [docs/design/M1-cekirdek.md](docs/design/M1-cekirdek.md) | M1 tasarım notu: veri yerleşimi, şema, ECS, anlık görüntü, hash |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Katmanlar, crate'ler, veri akışları, proje düzeni |
| [docs/MCP_API.md](docs/MCP_API.md) | Yapay zekâ arayüzü: 23 araçlık taslak |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Kilometre taşları, çıkış ölçütleri, model · efor önerileri, riskler |
| [CLAUDE.md](CLAUDE.md) | Motoru geliştiren ajan için çalışma kuralları |

## Özet

- **Dil:** Rust · **Grafik:** wgpu (WebGPU) · **Mimari:** şema önce, özel ECS, deterministik simülasyon
- **Oyun mantığı:** korumalı alanda WebAssembly, kayıpsız sıcak yükleme
- **Yapay zekâ:** motorun içinde yerleşik MCP sunucusu; kullanıcı kendi yapay zekâsını bağlar (öncelik Claude)
- **Arayüz:** hibrit — yapay zekâ kokpiti (göster ve söyle, etkinlik akışı, tek tıkla geri al) ve günümüz motorlarındaki gibi tam elle düzenleme; yapay zekâ bağlamak isteğe bağlı
- **Dosyalar:** her şey kanonik JSON; git dostu
- **Dağıtım:** ücretsiz, komisyonsuz; Steam ve web; tek paket, ek kurulum yok
- **Lisans:** açık kaynak, MIT + Apache-2.0
- **Komut satırı aracı:** `wen` · **crate öneki:** `wish_`
