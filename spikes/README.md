# S0 spikes

Throwaway prototypes for the S0 spikes in [docs/scope.md](../docs/scope.md) §12. Each spike is a standalone cargo project, and none of them is part of DIE's workspace. What carries forward is each spike's decision record in [docs/records/](../docs/records/), plus the tests that S4 ports into `die-core`.

| Spike | Directory | Decision record |
|---|---|---|
| S1 Renderer and platform | `s1-renderer/` | `docs/records/s1-renderer.md` |
| S2 Terminal engine wiring and helper process | `s2-term-engine/` | `docs/records/s2-term-engine.md` |
| S3 Terminal survival | `s3-fd-store/` | `docs/records/s3-fd-store.md` |
| S4 Core API and layout | `s4-core-api/` | `docs/records/s4-core-api.md` |
| S5 Evil oracle | `s5-evil-oracle/` | `docs/records/s5-evil-oracle.md` |

The rules every spike follows:

- **Toolchain.** Builds need only cargo and the Xcode Command Line Tools (D16). Nothing is installed system-wide, and downloads stay in scratch directories.
- **ghostel and Evil are GPL.** They're read or run as references only. None of their code is copied here.
