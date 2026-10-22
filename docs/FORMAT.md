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
