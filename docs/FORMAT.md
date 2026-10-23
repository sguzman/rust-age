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

## Palette gamma and integer fade interpolation

`src/palette.rs` now includes a six-bit gamma LUT following CE `colorGamma`: `pow(channel, gamma)` with 0..63 clamp and integer truncation. A separate `fade_step` function follows CE's signed integer interpolation between source/target 768-byte palettes. These are independently testable color primitives; frame-timed color cycling, real-time fade scheduling, sound callbacks and final SDL palette presentation are still unimplemented.

## Fallout 1 MAP header and square layers

`src/map.rs` implements a read-only subset of CE `src/game/map.cc` (version-19 map header and `square_load`). The original 236-byte big-endian header contains the 16-byte name, entering tile/elevation/rotation, per-map local/global variable counts, flags, script index and visit metadata. After global and local 32-bit variable tables, the file contains 10,000 big-endian 32-bit floor/roof tile words per present elevation (up to three). Flags 2/4/8 mark absent elevations; CE also clears bit 28 when importing the roof flag nibble. The parser exposes the byte offset of scripts but **does not parse or execute scripts, actors or object state**.

Synthetic tests exercise map version validation, optional elevation sections, variable-section offsets, truncation rejection and roof-flag normalization. Real Fallout 1 map files are not redistributed or yet tested against CE.

The read-only diagnostic `rust-age --inspect-map /path/to/Fallout maps/example.map` resolves the actual map via the user's master DAT/patch directory and prints map metadata and static layer availability. Substitute an existing game map path; it cannot start the map or interpret its scripts.

## Fallout 1 .PRO prototype prefix and subtypes

`src/prototype.rs` reads the big-endian common 12-byte PID/message/FID prefix and original per-object fields using the pinned CE `proto_read_protoSubNode` layout. It recognizes items, critters, scenery, walls, tiles and misc, including the original type-specific item/scenery field lengths. Weapon sound codes use the original trailing byte. Critter stats are **not decoded** and are retained as raw trailing bytes; remaining data is not evidence of fully parsed gameplay objects.

This is strictly a read-only, synthetic-test-backed reader. It does not instantiate entities, infer AI, interpret scripting or provide prototype behavior.

## Optional native MAP roof pass

`FloorPreview::render_layers(..., include_roofs=true)` runs a separate static roof pass after the floor, using the top 16-bit tile word, per-square CE roof projection (96 px above floor) and the original FRM transparency convention. The SDL2 MAP viewer switches the overlay using **R**. The preview deliberately omits CE roof egg masks, lighting and animated actor occlusion. This is for testing data alignment and geometry, not visual or gameplay parity.

## FRM animation timeline

`AnimationTimeline` selects looping FRM frame indices from **elapsed monotonic time**, independent of the SDL2 redraw loop. It uses the original `art_frame_fps` ten-FPS fallback for zero-rate assets and exact integer nanosecond arithmetic for frame boundaries. Negative header frame rates are currently rejected as invalid scheduling input. This is a pure timing primitive, not a gameplay event queue or automatic sprite animator.

## Native C++ / Rust differential geometry oracle

`tests/spatial_oracle.cc` is a small, self-contained C++ translation of CE's original integer tile and square projection formulas. `src/bin/spatial_probe.rs` emits the corresponding Rust coordinates. GitHub CI compiles the C++ oracle and compares more than 2,000 lines of hex, floor-square and roof-square output across several center positions, including even and odd columns. This verifies **numeric projection only**; it does not compare map files, visual lighting, world movement or game script semantics.

## CE critter prototype stat tables

The critter `.PRO` path now uses `critter_read_data` from the pinned CE `src/game/critter.cc`, reading **35 base stats**, **35 bonus stats**, **18 skills**, flags, body type, experience and kill type as original big-endian i32 fields. Synthetic fixtures assert a complete 0x19C-byte critter record, tail-byte preservation, and truncation rejection. This is a data reader only: stats calculations, combat, AI, actor construction and save compatibility remain unimplemented.

## PID-indexed prototype list lookup

CE `proto_list_str` selects a **one-based** `PID & 0xFFFFFF` row of `proto/<category>/<category>.lst`, then opens that row's `.PRO`. `prototype_list.rs` preserves row numbering and `GameAssets::prototype_by_pid` loads via master DAT/patch lookup, checking decoded PID identity. Filenames with traversal/separators are rejected. Synthetic fixtures cover list numbering, missing records and PID identity. No gameplay objects or prototype caching are constructed.

## Original MAP script-list records (no execution)

The read-only `ScriptTables` parser mirrors CE `scr_load` and `scr_read_ScriptSubNode`: five script categories, one 32-bit count per category, up to 16 serialized records per extent (including padding), and subtype-dependent spatial/timed fields. It retains flags, script indices and raw legacy pointer tokens without following pointers or running bytecode. The MAP reader exposes the byte offset where the object section starts. Synthetic tests cover empty maps, spatial/timed layouts, padded extents and malformed counts. Interpreter, state mutation and event execution remain unimplemented.

## Original MAP object-record prefix (read-only)

`src/object_data.rs` ports CE's `obj_read_obj` 18-word object prefix and `proto_read_protoUpdateData` inventory header plus type-specific update fields. Critter records include the 7-word combat metadata and HP/radiation/poison; item/scenery record lengths require a known `.PRO` subtype. This parser reads **one** object record and leaves inventory children and the full per-elevation object table to later work. It ignores serialized pointer tokens and normalizes the CE `0xCCCCCCCC` data-flags marker. No entity simulation or object rendering is implemented.

## Per-elevation object and recursive inventory tables

`src/object_tables.rs` now follows CE `obj_load_func` and `obj_load_obj`: a global top-level object count, one count for each of three elevations, and nested inventory children each preceded by quantity. It locates each object's PID at the original byte-44 offset and asks the caller for item/scenery prototype subtypes so the variable-width `proto_read_protoUpdateData` layout is consumed correctly. Strict size and recursion limits reject malicious inputs. Synthetic tests cover an object with a nested weapon, empty elevations, and malformed counts. This is **deserialization only**; no actors are instantiated or drawn.

## Full static MAP read boundary

`MapSquares::decode_objects_with` chains original 100×100 square layers, five script tables and per-elevation object/inventory records, returning the byte offset after the object section. It requires the caller to supply the correct item/scenery prototype subtype for variable-width updates. This is a read-only format boundary; script execution, actor placement, collision, rendering and save/restore are separate unfinished stages.

## CE static art FID category resolution

`ArtFid` separates the original FID's type (bits 24–27), 12-bit filename index, animation (16–23), weapon animation (12–15) and rotation (28–30). `GameAssets::direct_art_path` resolves static item/scenery/wall/tile/misc/interface/inventory/background/skilldex names via their own original `art/<dir>/<dir>.lst` file in the user's master bank. This mirrors the direct/default branch of pinned CE `art_get_name`. **Critter and head art are explicitly rejected** until CE's generated animation suffixes and alias handling are ported; this resolver must never silently invent those filenames.

## CE critter and head filename generation

`ArtFid` now mirrors CE `art_get_code` using named animation/weapon branches (stand, walk, dodge, throw, knockdown/death, weapon action, single-frame death), CE `head1`/`head2` talking-head suffix tables, and `.fr0`–`.fr6` directional critter filenames. `GameAssets::art_path_by_fid` chooses the critter archive for critter sprites and master archive for other art. Special critter death aliases use the explicit comma-separated alias in `critters.lst`; if absent, the reader deliberately errors rather than pretending to know CE's runtime fallback. Animation playback, actual asset comparison, cache aliases and actor rendering are still pending.

## CE make_path_func traversal (pure logic)

`src/pathfinding.rs` reproduces the original `anim.cc` 2000-slot open/closed search, visited-on-insert semantics, screen-coordinate `idist` heuristic, CE hex-neighbor order, original 50-point step cost and 10-point noncombat turning penalty. It returns a bounded direction sequence and checks obstacles via a caller-supplied passability callback (doors, collision and combat are not implemented). Tests cover nearest-neighbor parity, destination occupancy flag, detours, reachability and invalid tiles. This has not been compared against a compiled original C++ A* oracle and is not yet wired to runtime actor movement.

## Original object collision and multhex semantics (pure index)

`src/collision.rs` ports CE `obj_blocking_at` rules: only critter/scenery/wall FIDs block; hidden and `OBJECT_NO_BLOCK` actors are ignored; direct-tile occupants are checked before neighboring `OBJECT_MULTIHEX` occupants; current actor identity is excluded by per-elevation ordinal. The index is constructed from deserialized top-level MAP records, not nested inventory. A synthetic route test composes this index with the pure CE pathfinder. Door usability, runtime actor state, dynamic movement and collision updates remain pending.

## Differential CE C++ path oracle

`tests/path_oracle.cc` isolates the pinned CE `make_path_func` search under synthetic tile/blocker cases and `src/bin/path_probe.rs` emits the corresponding Rust sequences. CI compiles the C++ oracle and performs a byte-for-byte comparison to prevent subtle changes to tie ordering, turning cost, edge handling or route reconstruction. This verifies only the fixed numeric cases, not runtime collision, doors or gameplay motion.

## Serialized MAP variable tables

`MapHeader::decode_variables` now reads the exact CE `map_load_file` order: big-endian 32-bit global values immediately after the 236-byte header, followed by local values, then static squares and scripts. Synthetic mixed-sign tests verify slot ordering, truncation detection and correct offset into script/object data. This is read-only metadata; fresh-map `.GAM` overrides and all quest/VM mutations are not implemented.

## CE object sprite bounds (read-only)

`src/object_bounds.rs` translates CE `obj_bound` into safe integer math: the hex tile's native 32×16 anchor, FRM directional x/y offsets, object's own x/y offsets, center-bottom sprite registration, optional 1-pixel outline expansion, and floating objects positioned by saved sx/sy. Tests verify original integer coordinates, bounding boxes and invalid rotation/frame/tile handling. This is geometric placement only; original lighting, object ordering, alpha effects and painting remain unimplemented.

## CE-style asset lookup cache

`GameAssets` now keeps category `art/*.lst` bytes/parsed rows, `proto/*.lst` one-based indices and PID-decoded `.PRO` records in a per-installation in-memory cache. This avoids repeated DAT seeks/list scans per object or sprite, closer to original CE's art/prototype cache lifecycle. `clear_lookup_cache` explicitly refreshes these lookup caches after changing loose patches. Direct `read` remains uncached. Synthetic tests verify cached identity, invalidation and post-invalidation list changes. This is not a frame/image LRU and has no persistent storage.

## Composed static scene snapshot

`src/scene.rs` assembles the original MAP loading format into one immutable `StaticScene`: header, global/local variable arrays, floor/roof square layers, serialized script metadata, per-elevation objects and nested inventories, and derived CE collision. It resolves variable-width object updates through original PID-to-PRO lookups, leveraging cached asset lists. The `route` method applies the CE pathfinder against static object occupancy. This is a **read-only parsed scene**, not a gameplay world; no mutable player, clock, combat, scripts or map transitions.

## Shared static scene in CLI diagnostics

`--inspect-map` and `--inspect-map-route` now use the same `StaticScene::load` assembly, preventing divergent MAP variable/script/object decoding paths in their diagnostics. The route command still only reports a list of CE rotations; actor movement is not yet implemented.

## Unlit top-level object-sprite diagnostic

`src/object_preview.rs` uses decoded MAP top-level objects, CE art FID filename resolution, user-owned FRM pixels and CE `obj_bound` placement to paint static critter/item/scenery sprites on an indexed surface. It skips hidden objects, renders flat objects first, and caches each FID's decoded frames per pass. Synthetic integration asserts exact placed palette indices and transparent pixels. This is **not a finished CE scene renderer**: per-hex traversal order, lighting, egg masking, alpha effects, roof passes and animation are still unported.

## CE .INT procedure and string-table foundation (no VM execution)

`src/script_program.rs` reads CE's 42-byte preamble, big-endian procedure count, six-i32 (24-byte) `Procedure` table, size-prefixed identifier and static string tables. Procedure names are resolved from offsets relative to the identifier section; static strings are offset from the end of the static section's four-byte header. Raw string bytes are preserved without assuming UTF-8. An explicit 16-bit big-endian word reader supports future opcode dispatch. Bounds, count caps, malformed strings, and truncation are tested using synthetic script payloads. No interpreter state, bytecode execution, quest semantics, or runtime script loading is implemented.

## Complete CE core-interpreter opcode inventory

`src/int_opcode.rs` contains all 76 contiguous core interpreter opcodes 0x8000..0x804B, preserving symbolic names from pinned CE `src/int/intrpret.h`. The lookup is verified for coverage, boundaries and selected arithmetic/control instructions. It is a registry for future VM work, **not** script execution. External game opcodes from `intextra` and library modules remain separate future work.

## Linux native .INT metadata inspector

`rust-age --inspect-int <Fallout installation> scripts/<name>.int` now opens scripts via the existing master DAT/patch resolution and lists original procedure names, bytecode offsets and argument counts using the CE read-only decoder. It does not launch any procedure or mutate game state. This offers a user-owned data inspection bridge before interpreter execution is ported.

## First bounded CE typed-stack interpreter slice

`src/script_vm.rs` now executes only an explicitly allowlisted subset of original core instructions against synthetic `.INT` program bytes: typed integer/float/string-reference PUSH, stack POP/DUP/SWAP, numeric ADD/SUB/MUL/DIV, NEGATE/FLOOR, JUMP, NOOP and isolated stop/exit. This keeps CE's high-bit opcode dispatch and 10-bit handler index, original 0xC001 integer and 0xA001 float push tags, stack operand order, integer-add overflow promotion, and bound-checked file offsets. Each test has a step budget; unknown opcodes error instead of silently succeeding. Exit/stop lifecycle, external/native operations, callbacks, procedures, save state, and game integration are **not implemented**, so this is not yet a usable script VM.

## Bounded CE numeric and branch opcode coverage

The isolated `ScriptVm` now also implements numeric comparisons (EQ, NE, LE, GE, LT, GT), integer MOD, boolean AND/OR/NOT, integer bitwise AND/OR/XOR/NOT, and the original stack-consumption pattern of IF/WHILE. For static tests the interpreter retains CE integer operand order and distinguishes raw-bit NOT from `ProgramValue::isEmpty` branch conditions. String coercion, floating bitwise coercion, external pointer logic and full procedure call stacks remain explicitly unsupported.
