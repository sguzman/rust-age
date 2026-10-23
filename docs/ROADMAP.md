# Rust Age roadmap

Generated from [`progress.json`](progress.json) with `python3 tools/render_roadmap.py`.
Progress is the arithmetic mean of atomic task percentages. Completed tasks are retained in
the ledger but omitted from this display. Partial scaffold ≠ playable game.
Fallout 2 (M7) is gated on the accepted Fallout 1 Linux release (M6).

## 🧱 M0 — Foundation and Fallout 1 asset compatibility — `████████░░ 78%`
**Acceptance gate:** Reference pinned; native Rust diagnostic works; Fallout 1 archive and content readers verified.

### 🧭 P0 — Provenance, scaffolding and quality controls — `█████████░ 86%`

- 🔎 T004 — Establish C++ baseline comparison and reference fixtures — `█████░░░░░ 45%`

### 📦 P1 — Game-data archive, codec and filesystem — `███████░░░ 72%`

- 🧰 T101 — Check a user-supplied Fallout 1 installation — `████████░░ 80%`
- 🗜️ T102 — Translate CE LZSS decoder with synthetic unit cases — `████████░░ 80%`
- 🗃️ T103 — Implement CE DAT archive directory and entry extraction — `███████░░░ 68%`
- 🗂️ T104 — Implement loose-file patches, path resolution and case policy — `████████░░ 75%`
- 📄 T105 — Implement game configuration, messages and encodings — `███████░░░ 65%`
- 🧬 T106 — Decode FRM, PRO, map and script asset structures — `███████░░░ 66%`

## 🖼️ M1 — Pixel-faithful renderer and navigable maps — `█████░░░░░ 52%`
**Acceptance gate:** An original Fallout 1 location renders faithfully, with movement and transitions.

### 🎨 P2 — Art and rendering — `██████░░░░ 59%`

- 🌈 T201 — Port palettes, fades and color cycling — `██████░░░░ 55%`
- 🧟 T202 — Load FRM sprites, directions and animation timing — `███████░░░ 72%`
- 💡 T203 — Port tile/roof layering, compositing and lighting — `██████░░░░ 55%`
- 🪟 T204 — Build native Linux window, framebuffer scaling and input loop — `██████░░░░ 55%`

### 🗺️ P3 — Maps and world views — `████░░░░░░ 44%`

- 🧭 T301 — Implement original hex projection and map scrolling — `██████░░░░ 64%`
- 🚶 T302 — Port pathfinding, collision and actor movement — `████░░░░░░ 40%`
- 🚪 T303 — Port map/elevation load and spatial exits — `███████░░░ 71%`
- 🌍 T304 — Render world map, town selection and location discovery — `░░░░░░░░░░ 0%`

## 📟 M2 — Menus, UI, audio and cinematics — `░░░░░░░░░░ 0%`
**Acceptance gate:** Main menu, core game UI, music, effects and cinematics match CE.

### 🖱️ P4 — Menu, HUD and screens — `░░░░░░░░░░ 0%`

- 🎮 T401 — Translate CE mouse, input queue and shortcuts — `░░░░░░░░░░ 0%`
- 🎒 T402 — Port HUD, inventory, item inspection and action cursor — `░░░░░░░░░░ 0%`
- 🧑 T403 — Port character creation, stats, Pip-Boy and options — `░░░░░░░░░░ 0%`
- 💬 T404 — Port dialogue interface, fonts and localization presentation — `░░░░░░░░░░ 0%`

### 🔊 P5 — Sound and cinematics — `░░░░░░░░░░ 0%`

- 🎵 T501 — Port original ACM stream decoding and music — `░░░░░░░░░░ 0%`
- 🔉 T502 — Port effects, mixing, voice priority and timing — `░░░░░░░░░░ 0%`
- 🎬 T503 — Port video playback and audiovisual synchronization — `░░░░░░░░░░ 0%`
- 🎞️ T504 — Verify menu, intro, ending and cinematic sequences — `░░░░░░░░░░ 0%`

## 🧠 M3 — Fallout script VM and quest behaviors — `███░░░░░░░ 29%`
**Acceptance gate:** Original game scripts execute with CE-equivalent side effects and dialogue outcomes.

### 🔣 P6 — Script interpreter and native opcodes — `█████░░░░░ 45%`

- 📚 T601 — Inventory bytecode opcodes, stack layout and data — `█████░░░░░ 46%`
- ⚙️ T602 — Port interpreter, procedures and interpreter state — `█████░░░░░ 54%`
- 🔌 T603 — Port engine opcodes, callbacks and event dispatch — `████░░░░░░ 36%`
- 🧪 T604 — Golden-script differential tests versus CE — `████░░░░░░ 44%`

### 📜 P7 — Quests, dialogue and triggers — `█░░░░░░░░░ 12%`

- 🗣️ T701 — Port dialogue branches and NPC reactions — `░░░░░░░░░░ 0%`
- 🚩 T702 — Port map scripts, spatial triggers and timers — `███░░░░░░░ 29%`
- 🧮 T703 — Port global/local state and quest flags — `██░░░░░░░░ 20%`
- 🤝 T704 — Port companion scripts and party interactions — `░░░░░░░░░░ 0%`

## ⚔️ M4 — Gameplay and combat parity — `░░░░░░░░░░ 1%`
**Acceptance gate:** Characters, inventory, rules, turn-based combat and AI produce original outcomes.

### 🧬 P8 — Character and item rules — `░░░░░░░░░░ 0%`

- 💪 T801 — Port SPECIAL, stats, skills, perks and traits — `░░░░░░░░░░ 0%`
- 🎲 T802 — Port RNG, skill checks and progression — `░░░░░░░░░░ 0%`
- 🎒 T803 — Port inventory/equipment, trade and item effects — `░░░░░░░░░░ 0%`
- ☢️ T804 — Port damage, healing, poison and radiation — `░░░░░░░░░░ 0%`

### 🔫 P9 — Combat and AI — `░░░░░░░░░░ 0%`

- 🎯 T901 — Port AP economy, hit chance, weapons and damage — `░░░░░░░░░░ 0%`
- 🧠 T902 — Port NPC combat AI and targeting — `░░░░░░░░░░ 0%`
- 💥 T903 — Port criticals, deaths and animations — `░░░░░░░░░░ 0%`
- 🔬 T904 — Differential fixed-seed combat scenarios — `░░░░░░░░░░ 0%`

### 🌐 P10 — Simulation and progression — `░░░░░░░░░░ 3%`

- ⏰ T1001 — Port time, rest and random encounters — `█░░░░░░░░░ 10%`
- 🚶 T1002 — Port world travel and location state — `░░░░░░░░░░ 0%`
- 🎭 T1003 — Port NPC schedule, hostility and reputation — `░░░░░░░░░░ 0%`
- 🏁 T1004 — Port endings, fail states and victory conditions — `░░░░░░░░░░ 0%`

## 💾 M5 — Complete Fallout 1 and original save compatibility — `░░░░░░░░░░ 0%`
**Acceptance gate:** Complete the Fallout 1 campaign on Linux using game files supplied by the player.

### 💽 P11 — Save/load and recovery — `░░░░░░░░░░ 0%`

- 📝 T1101 — Implement original save file serialization — `░░░░░░░░░░ 0%`
- 📖 T1102 — Load CE and original save games — `░░░░░░░░░░ 0%`
- 🔁 T1103 — Round-trip maps, inventory, quests and scripts — `░░░░░░░░░░ 0%`
- 🛟 T1104 — Reject corrupted saves safely — `░░░░░░░░░░ 0%`

### 🚀 P12 — Full campaign integration — `░░░░░░░░░░ 0%`

- 🏠 T1201 — Start game and reach Vault 13 — `░░░░░░░░░░ 0%`
- 🚰 T1202 — Verify water chip quest and consequences — `░░░░░░░░░░ 0%`
- 🏜️ T1203 — Verify critical towns, companions and late-game branches — `░░░░░░░░░░ 0%`
- 🏆 T1204 — Complete campaign, final cinematics and credits — `░░░░░░░░░░ 0%`

## 🧪 M6 — Strict Fallout 1 CE parity and Linux MVP release — `░░░░░░░░░░ 0%`
**Acceptance gate:** Full CE behavior comparison and verified Linux release; Fallout 2 remains deferred until this passes.

### 🔬 P13 — Differential QA and legal release audit — `░░░░░░░░░░ 0%`

- 🧷 T1301 — Compare asset decoding and file resolution against CE — `░░░░░░░░░░ 0%`
- 📐 T1302 — Compare rendering, timing, RNG and script behavior — `░░░░░░░░░░ 0%`
- 🕹️ T1303 — Automate gameplay/saving regression scenarios — `░░░░░░░░░░ 0%`
- 🔐 T1304 — Audit source provenance, attribution and asset exclusion — `░░░░░░░░░░ 0%`

### 🐧 P14 — EndeavourOS native release and dogfooding — `░░░░░░░░░░ 0%`

- 📦 T1401 — Build and document reproducible Linux binaries — `░░░░░░░░░░ 0%`
- 🖥️ T1402 — Test Wayland/X11, window focus and scaling — `░░░░░░░░░░ 0%`
- 🛠️ T1403 — Complete diagnostics, configuration and user errors — `░░░░░░░░░░ 0%`
- 🚦 T1404 — Verify entire game and inaugurate native Rust release — `░░░░░░░░░░ 0%`

## ☢️ M7 — Fallout 2 port — strictly after Fallout 1 release — `░░░░░░░░░░ 0%`
**Acceptance gate:** Do not begin before M6 is accepted; keep existing fallout2-ce fork reference-only.

### 🔍 P15 — Fallout 2 CE delta audit — `░░░░░░░░░░ 0%`

- 📌 T1501 — Pin Fallout 2 CE version and licensing — `░░░░░░░░░░ 0%`
- 🧭 T1502 — Inventory engine, VM, scripts and data differences — `░░░░░░░░░░ 0%`
- 📦 T1503 — Implement Fallout 2 game-data compatibility — `░░░░░░░░░░ 0%`
- 🧩 T1504 — Reuse validated Rust core without premature convergence — `░░░░░░░░░░ 0%`

### 🚀 P16 — Fallout 2 game and release — `░░░░░░░░░░ 0%`

- ⚔️ T1601 — Port Fallout 2 gameplay/AI and party differences — `░░░░░░░░░░ 0%`
- 🏙️ T1602 — Port Fallout 2 UI/maps/quests/encounters — `░░░░░░░░░░ 0%`
- 💾 T1603 — Verify original saves and entire Fallout 2 campaign — `░░░░░░░░░░ 0%`
- 🐧 T1604 — Complete Fallout 2 Linux parity release — `░░░░░░░░░░ 0%`

## Progress accounting

- Every active milestone, phase and task touched by a progress report gets a percent bar.
- Completed tasks (100%) disappear from progress reports and this view; historical evidence remains in `progress.json`.
- Do not assign 100% without evidence; differentiate code written, CI-tested and game-tested.
- Do not commit original game data, private locations, accounts or personal conversations.
