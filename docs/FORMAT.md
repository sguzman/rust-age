# 📦 Fallout 1 DAT format and Rust decoder

Reference: [CE `src/plib/db/db.cc`](https://github.com/alexbatalov/fallout1-ce/blob/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b/src/plib/db/db.cc) and [`assoc.cc`](https://github.com/alexbatalov/fallout1-ce/blob/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b/src/plib/assoc/assoc.cc).

Fallout 1 archive structure is **not** Fallout 2's later DAT format. This parser follows CE's big-endian associative arrays at the start of the file: a root directory header, each directory name and opaque serialized value, then a second associative array of file names and 16-byte directory entries per directory.

The wire integer fields are big-endian signed 32-bit. Each associative header has `size, max, datasize, legacy-pointer` fields. Each named entry has one name-length byte, the name, then its configured opaque value. File entry data contains `flags, offset, uncompressed_length, packed_length`.

Three CE storage modes are decoded:
- `flags == 0` or `(flags & 0xF0) == 0x10`: whole-file LZSS, using the final entry field as compressed-byte count.
- `(flags & 0xF0) == 0x20`: uncompressed bytes at the supplied archive offset.
- `(flags & 0xF0) == 0x40`: two-byte big-endian chunk headers; high bit denotes raw chunk, otherwise compressed with LZSS.

The Rust parser rejects malformed sizes, invalid offsets and decompressed-size mismatches. It exposes a read-only API and never writes a DAT file. Synthetic fixtures cover each storage mode; **real Fallout game-data differential tests against CE are still required**. Do not commit real copyrighted game archives or extracted contents to this repository.

## Default installed-game file selection

Fallout 1 CE initializes two separate databases (`master.dat` and `critter.dat`) and usually selects `master.dat`; critter art explicitly switches to `critter.dat`. Both databases default their loose-file patch directory to `data/` via `gconfig.cc`. CE's Linux compatibility layer searches directory entries case-insensitively and converts backslashes to native separators.

The Rust `GameAssets` reader follows that common read-only path, with a `Bank` selection, DAT access, and patch-before-archive resolution. This is not yet full CE configuration/path behavior: custom `fallout.cfg` paths, special `@` direct-file syntax, writes, and all legacy path edge cases remain incomplete.

## FRM sprite format

`src/frm.rs` ports the CE art data loader at a primitive level. A 62-byte header contains the version, frames-per-second, action frame, frame count, x/y offsets for six rotations, per-rotation data offsets, and size. Each subframe has a 12-byte header: width, height, data length, x/y motion offsets, then 8-bit palette-indexed pixels. Several directions may share the same frame-data offset; Rust retains that sharing at parse time by caching offset-decoded frame sequences.

This is strictly **asset decoding**, not native rendering or original palette parity. Synthetic fixtures cover basic reading, shared rotations, and truncated files. Real FRM differential verification is pending.

## Original Fallout palette and lookup table

`src/palette.rs` is adapted from [CE `src/plib/color/color.cc`](https://github.com/alexbatalov/fallout1-ce/blob/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b/src/plib/color/color.cc). The `color.pal` file begins with 256 RGB colors, one byte per component, valid original range 0..63. CE zeros invalid colors and separately records which indices are mapped. The next 32,768 bytes encode the original 5:5:5 RGB-to-palette-index lookup table.

The Rust parser retains both source tables. Its 8-bit conversion is only a preview (linear 6-bit rescaling); it is **not** a port of CE gamma tables, color cycling, animated fades, or pixel-accurate compositing.

## Fallout 1 configuration and MSG localization

`src/config.rs` ports a read-only subset of CE `config.cc`: section headers, trimmed key=value fields, semicolon comments, case-insensitive lookup, and original Fallout 1 installation defaults from `gconfig.cc`. Saving, CLI overrides and complete CE preference coverage are not yet implemented.

`src/message.rs` ports a read-only subset of CE `message.cc`: `{number}{audio}{text}` fields, LF removal within a braced field, 1023-byte field limit, sorted IDs and duplicate replacement. The localized text remains in its original encoded bytes, **not** blindly converted to UTF-8. Integrating the language-dependent `text/<language>/` lookup into the running engine is future work.

### Configured Fallout 1 asset filenames and patch paths

The asset resolver now reads `fallout.cfg` if present, merges its `[system]` entries over CE defaults and honors separate `master_dat`, `critter_dat`, `master_patches` and `critter_patches`. This retains CE's patch-before-archive behavior. Synthetic integration tests cover case-insensitive config filenames and separate patch directories, without game data. Filesystem access is read-only and confined to the supplied installation for normal relative paths. Full CE config semantics and nonstandard absolute/direct-file lookup remain pending.

## 8-bit indexed framebuffer and offline visual diagnostics

`src/framebuffer.rs` follows the core CE `src/plib/gnw/grbuf.cc` distinction between plain pixel copies and `trans_buf_to_buf` with transparent palette index zero. It offers safe signed-coordinate clipping, opaque fill, indexed FRM blit, and a P6 (binary RGB) PPM exporter using the player's original `color.pal`. Unit tests cover clipping, transparent/opaque zero, malformed source dimensions and exact preview output bytes.

`--export-frm-preview <install> <master|critter> <relative FRM path> <direction 0..5> <frame index> <destination.ppm>` is a diagnostic. It loads the original palette and sprite from the user-provided Fallout installation and exports one unlit 2D frame. It never embeds original game content in source code or releases. The output is not CE gamma-adjusted, does not implement depth/roof rendering, and is not a native window.
