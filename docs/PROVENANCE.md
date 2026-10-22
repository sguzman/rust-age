# 🧭 Source baseline and licenses

Fallout 1 CE: [alexbatalov/fallout1-ce](https://github.com/alexbatalov/fallout1-ce), baseline [`0609bcfd0ec40ff0571d0f57fab2821eb461dc8b`](https://github.com/alexbatalov/fallout1-ce/commit/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b) (Jan 15, 2025). User's [fallout1-ce](https://github.com/sguzman/fallout1-ce) and [fallout2-ce](https://github.com/sguzman/fallout2-ce) forks are read-only **references**; neither is edited by this project. Fallout 2 translation is deferred until the complete Fallout 1 acceptance gate.

Rust Age aims to translate CE C++ behavior, with C++ kept external and cross-checked by deterministic tests. Rust Age is not a clean-room implementation. `src/lzss.rs` translates `src/plib/db/lzss.cc`, with its origin credited here; further translated files must carry their source provenance.

Fallout 1 CE uses **Sustainable Use License 1.0**. Derived code must honor attribution, notices and limitations including noncommercial-only distribution. The project's original empty-repository MIT file is archived in [INITIAL-LICENSE.md](INITIAL-LICENSE.md); it **cannot** grant MIT rights to upstream CE-derived work. The repository's primary [LICENSE](../LICENSE) is therefore SUL 1.0.

No Bethesda/Interplay-owned content, data archives, sprites, videos, music, scripts or installation files belong in the repository. Players supply originals from their own installation. No affiliation with Bethesda, Interplay or original developers is claimed.
