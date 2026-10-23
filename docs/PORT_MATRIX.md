# 🧩 Community Edition C++ → Rust port matrix

**Source:** [Fallout 1 Community Edition pinned baseline](https://github.com/alexbatalov/fallout1-ce/commit/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b).
Generated from [`port-matrix.json`](port-matrix.json); do not manually edit this document.
**Core compilation units:** 108. **Complete:** 0. **Partial:** 22. **Not ported:** 86.
Partial means a narrow reader/function exists, not that the entire C++ implementation has been replaced.
The archive is not yet playable; the source matrix is not a gameplay-progress percentage.

## src (5)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/audio_engine.cc` | — | ⚪ Not ported |
| `src/fps_limiter.cc` | — | ⚪ Not ported |
| `src/movie_lib.cc` | — | ⚪ Not ported |
| `src/platform_compat.cc` | `src/assets.rs` | 🟡 Partial |
| `src/pointer_registry.cc` | — | ⚪ Not ported |

## src/game (65)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/game/actions.cc` | — | ⚪ Not ported |
| `src/game/amutex.cc` | — | ⚪ Not ported |
| `src/game/anim.cc` | `src/pathfinding.rs`, `src/animation.rs`, `src/actor_route.rs` | 🟡 Partial |
| `src/game/art.cc` | `src/frm.rs`, `src/art_list.rs`, `src/art_fid.rs`, `src/assets.rs` | 🟡 Partial |
| `src/game/automap.cc` | — | ⚪ Not ported |
| `src/game/bmpdlog.cc` | — | ⚪ Not ported |
| `src/game/cache.cc` | — | ⚪ Not ported |
| `src/game/combat.cc` | — | ⚪ Not ported |
| `src/game/combatai.cc` | — | ⚪ Not ported |
| `src/game/config.cc` | `src/config.rs` | 🟡 Partial |
| `src/game/credits.cc` | — | ⚪ Not ported |
| `src/game/critter.cc` | `src/prototype.rs` | 🟡 Partial |
| `src/game/cycle.cc` | — | ⚪ Not ported |
| `src/game/display.cc` | — | ⚪ Not ported |
| `src/game/editor.cc` | — | ⚪ Not ported |
| `src/game/elevator.cc` | — | ⚪ Not ported |
| `src/game/endgame.cc` | — | ⚪ Not ported |
| `src/game/fontmgr.cc` | — | ⚪ Not ported |
| `src/game/game.cc` | `src/assets.rs`, `src/game_globals.rs` | 🟡 Partial |
| `src/game/gconfig.cc` | `src/assets.rs` | 🟡 Partial |
| `src/game/gdebug.cc` | — | ⚪ Not ported |
| `src/game/gdialog.cc` | — | ⚪ Not ported |
| `src/game/gmemory.cc` | — | ⚪ Not ported |
| `src/game/gmouse.cc` | — | ⚪ Not ported |
| `src/game/gmovie.cc` | — | ⚪ Not ported |
| `src/game/graphlib.cc` | — | ⚪ Not ported |
| `src/game/gsound.cc` | — | ⚪ Not ported |
| `src/game/heap.cc` | — | ⚪ Not ported |
| `src/game/intface.cc` | — | ⚪ Not ported |
| `src/game/inventry.cc` | — | ⚪ Not ported |
| `src/game/item.cc` | — | ⚪ Not ported |
| `src/game/light.cc` | — | ⚪ Not ported |
| `src/game/lip_sync.cc` | — | ⚪ Not ported |
| `src/game/loadsave.cc` | — | ⚪ Not ported |
| `src/game/main.cc` | — | ⚪ Not ported |
| `src/game/mainmenu.cc` | — | ⚪ Not ported |
| `src/game/map.cc` | `src/map.rs`, `src/object_tables.rs`, `src/scene.rs`, `src/script_locals.rs` | 🟡 Partial |
| `src/game/message.cc` | `src/message.rs` | 🟡 Partial |
| `src/game/moviefx.cc` | — | ⚪ Not ported |
| `src/game/object.cc` | `src/object_data.rs`, `src/object_tables.rs`, `src/collision.rs`, `src/object_bounds.rs`, `src/object_preview.rs`, `src/runtime_scene.rs` | 🟡 Partial |
| `src/game/options.cc` | — | ⚪ Not ported |
| `src/game/palette.cc` | — | ⚪ Not ported |
| `src/game/party.cc` | — | ⚪ Not ported |
| `src/game/perk.cc` | — | ⚪ Not ported |
| `src/game/pipboy.cc` | — | ⚪ Not ported |
| `src/game/protinst.cc` | — | ⚪ Not ported |
| `src/game/proto.cc` | `src/prototype.rs`, `src/prototype_list.rs` | 🟡 Partial |
| `src/game/queue.cc` | `src/event_queue.rs`, `src/script_timer.rs` | 🟡 Partial |
| `src/game/reaction.cc` | — | ⚪ Not ported |
| `src/game/roll.cc` | `src/random_roll.rs` | 🟡 Partial |
| `src/game/scripts.cc` | `src/script_data.rs`, `src/script_list.rs`, `src/script_binding.rs`, `src/script_procedures.rs`, `src/assets.rs`, `src/script_lifecycle.rs`, `src/script_locals.rs`, `src/script_clock.rs`, `src/script_timer.rs` | 🟡 Partial |
| `src/game/select.cc` | — | ⚪ Not ported |
| `src/game/selfrun.cc` | — | ⚪ Not ported |
| `src/game/sfxcache.cc` | — | ⚪ Not ported |
| `src/game/sfxlist.cc` | — | ⚪ Not ported |
| `src/game/skill.cc` | — | ⚪ Not ported |
| `src/game/skilldex.cc` | — | ⚪ Not ported |
| `src/game/stat.cc` | — | ⚪ Not ported |
| `src/game/textobj.cc` | — | ⚪ Not ported |
| `src/game/tile.cc` | `src/hex.rs` | 🟡 Partial |
| `src/game/trait.cc` | — | ⚪ Not ported |
| `src/game/version.cc` | — | ⚪ Not ported |
| `src/game/wordwrap.cc` | — | ⚪ Not ported |
| `src/game/worldmap.cc` | — | ⚪ Not ported |
| `src/game/worldmap_walkmask.cc` | — | ⚪ Not ported |

## src/int (17)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/int/audio.cc` | — | ⚪ Not ported |
| `src/int/audiof.cc` | — | ⚪ Not ported |
| `src/int/datafile.cc` | — | ⚪ Not ported |
| `src/int/dialog.cc` | — | ⚪ Not ported |
| `src/int/export.cc` | — | ⚪ Not ported |
| `src/int/intlib.cc` | — | ⚪ Not ported |
| `src/int/intrpret.cc` | `src/script_program.rs`, `src/int_opcode.rs`, `src/script_vm.rs`, `src/script_procedures.rs` | 🟡 Partial |
| `src/int/memdbg.cc` | — | ⚪ Not ported |
| `src/int/mousemgr.cc` | — | ⚪ Not ported |
| `src/int/movie.cc` | — | ⚪ Not ported |
| `src/int/nevs.cc` | — | ⚪ Not ported |
| `src/int/pcx.cc` | — | ⚪ Not ported |
| `src/int/region.cc` | — | ⚪ Not ported |
| `src/int/share1.cc` | — | ⚪ Not ported |
| `src/int/sound.cc` | — | ⚪ Not ported |
| `src/int/widget.cc` | — | ⚪ Not ported |
| `src/int/window.cc` | — | ⚪ Not ported |

## src/int/support (1)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/int/support/intextra.cc` | `src/script_native_vars.rs`, `src/game_globals.rs`, `src/script_locals.rs`, `src/script_clock.rs`, `src/script_random.rs` | 🟡 Partial |

## src/plib/assoc (1)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/plib/assoc/assoc.cc` | `src/dat.rs` | 🟡 Partial |

## src/plib/color (1)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/plib/color/color.cc` | `src/palette.rs` | 🟡 Partial |

## src/plib/db (2)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/plib/db/db.cc` | `src/dat.rs`, `src/assets.rs` | 🟡 Partial |
| `src/plib/db/lzss.cc` | `src/lzss.rs` | 🟡 Partial |

## src/plib/gnw (16)

| C++ compilation unit | Rust counterpart | Status |
|---|---|---|
| `src/plib/gnw/button.cc` | — | ⚪ Not ported |
| `src/plib/gnw/debug.cc` | — | ⚪ Not ported |
| `src/plib/gnw/dxinput.cc` | — | ⚪ Not ported |
| `src/plib/gnw/gnw.cc` | — | ⚪ Not ported |
| `src/plib/gnw/grbuf.cc` | `src/framebuffer.rs` | 🟡 Partial |
| `src/plib/gnw/input.cc` | — | ⚪ Not ported |
| `src/plib/gnw/intrface.cc` | — | ⚪ Not ported |
| `src/plib/gnw/kb.cc` | — | ⚪ Not ported |
| `src/plib/gnw/memory.cc` | — | ⚪ Not ported |
| `src/plib/gnw/mouse.cc` | — | ⚪ Not ported |
| `src/plib/gnw/rect.cc` | — | ⚪ Not ported |
| `src/plib/gnw/svga.cc` | — | ⚪ Not ported |
| `src/plib/gnw/text.cc` | — | ⚪ Not ported |
| `src/plib/gnw/touch.cc` | — | ⚪ Not ported |
| `src/plib/gnw/vcr.cc` | — | ⚪ Not ported |
| `src/plib/gnw/winmain.cc` | — | ⚪ Not ported |

## Method

The manifest covers C++ compilation units in the pinned CE root CMake `target_sources` list. Header-only code, third-party codecs, platform-specific glue, behavioral overlap and game-data verification require separate audits. File-level completion is intentionally stricter than task percentage accounting; do not count partial ports as complete.
