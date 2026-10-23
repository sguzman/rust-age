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
cargo run -- --inspect-map /path/to/Fallout maps/example.map
cargo run -- --inspect-pro /path/to/Fallout proto/items/example.pro
cargo run -- --export-frm-preview /path/to/Fallout master art/items/example.frm 0 0 preview.ppm
python3 tools/render_roadmap.py --check
```

Replace the example FRM path with an existing sprite from your own Fallout 1 installation. The checker verifies only paths; the index validator opens both archives; sprite preview saves an unlit RGB PPM from original asset data, not a full frame or playable window. None of these commands proves game parity.

### Static MAP floor preview (read-only)

The map preview loads the original `MAP` floor IDs, resolves `art/tiles/tiles.lst`, and draws the matching original FRM files into a 640×380 indexed framebuffer before exporting a PPM. It is a diagnostic and **not** the game's full map renderer: no lighting, scripts, interactive movement, roofs, actors, or quest progression.

```sh
cargo run -- --export-map-floor-preview /path/to/Fallout maps/example.map floor.ppm
cargo run -- --export-map-roof-preview /path/to/Fallout maps/example.map roofs.ppm
```

Replace `maps/example.map` with a real map path from the original game installation. No extracted game assets belong in the repository.

### Optional native SDL2 floor MAP window

This is a **static, read-only floor preview**, not a playable game. It uses the original map's entering tile, the local MAP + LST + FRM content pipeline, and the original VGA palette. It has no object/roof compositing, scripts or lighting; no original game data is included in the build.

```sh
cargo run --features native-window --bin rust-age-map-view -- /path/to/Fallout maps/example.map
```

Press **R** to toggle the unlit roof layer, and **Escape** to close. Resizing scales the original 640×380 framebuffer. This is still not a complete roof implementation: no egg mask, lighting or game simulation. Replace the example path with a map actually present in your installation.

### Optional native SDL2 sprite window

On EndeavourOS, install the standard `sdl2` development package if it is not already installed. The read-only SDL2 viewer is an **optional feature**; basic diagnostics do not require SDL2:

```sh
cargo run --features native-window --bin rust-age-view -- /path/to/Fallout master art/items/example.frm
```

Choose a real FRM path from your own game installation. Use Left/Right to switch frames, Up/Down to switch directions, **Space to play/pause the original sprite animation independently of window redraws**, and Escape to close. SDL2 uses the available Linux display backend (Wayland where supported, otherwise X11/XWayland). This is **not** a playable map or full game.

## Progress and issues

[📍 Roadmap](docs/ROADMAP.md) · [🔢 Progress ledger](docs/progress.json) · [🧩 C++→Rust port matrix](docs/PORT_MATRIX.md) · [📋 GitHub tracking queue](docs/ISSUES.md).

Milestone/phase progress is aggregated from atomic tasks. Completed tasks disappear from progress reports, but their evidence remains in the ledger. No percentage is a claim that the game is playable.

## Licensing

**Sustainable Use License 1.0** governs this direct derivative of Fallout 1 CE (see [LICENSE](LICENSE) and [provenance](docs/PROVENANCE.md)). This is source-available **but not OSI-open-source**, and noncommercial limitations apply. The empty repo was initially created with MIT text; it is preserved as [historical initialization context](docs/INITIAL-LICENSE.md), not as permission to relicense CE-derived work. Neither Bethesda nor Community Edition endorses this project.
