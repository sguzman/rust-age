# Rust Age ☢️🦀

**A strict native Rust port of Fallout 1 Community Edition, not a new game and not a C++ wrapper.** Fallout 2 follows only when Fallout 1 is working and verified.

![Rust Age iron-oxide red identity](assets/rust-age-color.svg)

**Project color:** `#E05B4C` · `oklch(64% 0.17 29)` — [perceptual choice](docs/COLOR.md).

> **Not playable yet.** This foundation currently provides a basic installation-path checker, a DAT archive index and extractor, master/critter patch-overlay lookup, and an FRM sprite decoder, palette tables, and original configuration/localized MSG readers. Only an offline indexed framebuffer and PPM sprite preview exist; no playable renderer, game loop or script interpreter exists.

## Source and operating model

The implementation is a source-level translation of [Fallout 1 CE](https://github.com/alexbatalov/fallout1-ce) pinned at [`0609bcfd0ec40ff0571d0f57fab2821eb461dc8b`](https://github.com/alexbatalov/fallout1-ce/commit/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b). Your [fallout1-ce fork](https://github.com/sguzman/fallout1-ce) is read-only reference material, not the target of development. [Fallout 2 CE](https://github.com/sguzman/fallout2-ce) remains read-only and out of scope until Fallout 1 ships.

Users supply their own legally obtained Fallout 1 installation. Rust Age loads original assets locally; no commercial game data or Windows executable is included in the repository.

## Development

```sh
cargo test --all-targets
cargo run -- --check-installation /path/to/Fallout
cargo run -- --verify-installation /path/to/Fallout
cargo run -- --inspect-dat /path/to/Fallout/master.dat
cargo run -- --inspect-frm /path/to/Fallout master art/items/example.frm
cargo run -- --export-frm-preview /path/to/Fallout master art/items/example.frm 0 0 preview.ppm
python3 tools/render_roadmap.py --check
```

Replace the example FRM path with an existing sprite from your own Fallout 1 installation. The checker verifies only paths; the index validator opens both archives; sprite preview saves an unlit RGB PPM from original asset data, not a full frame or playable window. None of these commands proves game parity.

## Progress and issues

[📍 Roadmap](docs/ROADMAP.md) · [🔢 Progress ledger](docs/progress.json) · [🧩 C++→Rust port matrix](docs/PORT_MATRIX.md) · [📋 GitHub tracking queue](docs/ISSUES.md).

Milestone/phase progress is aggregated from atomic tasks. Completed tasks disappear from progress reports, but their evidence remains in the ledger. No percentage is a claim that the game is playable.

## Licensing

**Sustainable Use License 1.0** governs this direct derivative of Fallout 1 CE (see [LICENSE](LICENSE) and [provenance](docs/PROVENANCE.md)). This is source-available **but not OSI-open-source**, and noncommercial limitations apply. The empty repo was initially created with MIT text; it is preserved as [historical initialization context](docs/INITIAL-LICENSE.md), not as permission to relicense CE-derived work. Neither Bethesda nor Community Edition endorses this project.
