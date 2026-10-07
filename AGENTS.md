# AGENTS.md

Guidance for AI coding agents (and humans) working on **Rex Ruckus: Meltdown**, a fast 90s-style
3D shooter inspired by the Build-engine classics, written in Rust with Bevy 0.19.

## Ground rules

- **Original IP only.** No code, maps, art, sounds or names from Duke Nukem 3D or the Build
  engine. Content is procedural or original; any third-party asset must be CC0 and listed with
  its source URL in `CREDITS.md`.
- Licence is GPL-3.0-or-later; new files fall under it.

## Layout

| Path | What lives there |
|---|---|
| `crates/core` (`rr-core`) | Pure simulation: sector maps, collision, movement, mechanics, combat, actors; triggers, quakes, hazards (`hazard.rs`), gag props (`props.rs`), infighting and boss phases, automap (`automap.rs`). **No Bevy.** Z-up, metres; headings in radians, 0 = east, CCW. |
| `crates/game` (`rr-game`, bin `rex-ruckus`) | Bevy front end: rendering, input, HUD, demo playback, `src/automap.rs` (Tab overlay); `src/menu/` holds the menu, pause, options, controls and stats screens. Convert between core and Bevy coordinates **only** in `src/coords.rs`. |
| `crates/tools` (`rr-tools`) | `validate`, `info` and `render-svg [--ids]` for level files. |
| `assets/episode.ron` | The episode: its name and the levels in play order. |
| `assets/fonts/` | The UI/HUD font (CC0 TTF). |
| `assets/levels/*.ron` | Levels (authored doors are open; the game closes them). |
| `assets/defs/*.ron` | Weapon and enemy stats; `models.ron` maps enemies, viewmodel weapons, pickups and gag props to models (scale, placement, clip names, muzzles). |
| `assets/models/` | glTF (`.glb`) for enemies, viewmodel weapons, pickups and gag props (`props/`; a prop kind without a model gets a code-built fallback): original models built from `scripts/models/`, the rest CC0. The boot, kick leg, pipe-bomb fist and switch panels stay code-built. |
| `assets/demo/*.ron` | Scripted demo runs (`--demo`). |
| `assets/sounds/` | `bank.ron` (event to sound map), `synth.ron` recipes, `synth/` rendered output, `cc0/` curated CC0 recordings. |
| `assets/music/` | Level music (OGG). |
| `assets/quips/` | Hero voice quips (OGG) and `quips.ron` (texts). |
| `scripts/` | `gen-quips.sh` + `quips_kokoro.py` (Kokoro voice quips), `import-sfx.sh` (CC0 import via sox), `import-models.sh` (CC0 model import; uses `blend_to_glb.py`, a Blender conversion for models shipped only as `.blend`), `build-models.sh` + `models/` (original models as headless-Blender Python scripts, sharing `models/rrkit.py`), `record-demo.sh` (README demo). |
| `installer/` | Inno Setup script for the Windows installer. |

Assets are found via `$RR_ASSETS`, else the workspace `assets/`, else `assets/` next to the exe.

## Build and check

Linux needs `libasound2-dev libudev-dev pkg-config` (plus Wayland/xkbcommon dev packages on CI).
Run all of these before opening a PR; CI runs the same:

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo run -p rr-tools -- validate assets/levels/*.ron

CI uses the latest stable Rust, so its clippy can flag lints an older local toolchain misses.
`.github/workflows/release.yml` builds the Windows installer and the Linux tarball
(`scripts/package-linux.sh`) on every PR, and publishes both on `v*` tags.

## Gotchas

- `rr-core`'s `glam` must be the exact version Bevy uses (shared `Vec2`/`Vec3`). Bump it only
  together with Bevy; Dependabot is told to ignore glam minor/major bumps.
- Keep simulation logic in `rr-core` with unit/property tests; `rr-game` systems should stay
  thin. Headless Bevy tests live in `crates/game/tests/` (see `restart_sim.rs` for the setup).
- Audio assets are generated or curated offline and their outputs are committed (`rr-tools synth`,
  `scripts/gen-quips.sh`, `scripts/import-sfx.sh`). `rr-tools validate` enforces sound-bank
  coverage, quip files, level music and `CREDITS.md` provenance. The quip voice is Kokoro-82M
  `am_onyx` at a pinned revision (a deliberate exception to CC0-only, see `CREDITS.md`); don't
  swap the model or voice without re-checking its licence.
- Models are curated or built offline and committed (`scripts/import-models.sh`,
  `scripts/build-models.sh`); don't edit the `.glb` files. Original models are changed in their
  `scripts/models/*.py` and rebuilt; pass `--preview DIR` for turnaround and per-clip contact
  sheets to check them before running the game.
  `rr-tools validate` enforces model coverage, clip and bone names, the triangle budget and
  `CREDITS.md` provenance. glTF models may ship lights (they are despawned on load), and Quaternius
  armatures carry a x100 scale, so attach placements in `models.ron` are in world units. The scripted
  models are built at real size (`scale: 1.0`) with glTF joint frames equal to Blender's bone frames.
- A full `cargo test --workspace` can run out of memory at default parallelism on small machines;
  cap it with `cargo test --workspace -j 4`.
- Headless tests do not add `AudioPlugin`; audio systems are gated on the `SoundBank` resource,
  so tests without audio need no changes.
- Bevy debug builds are large; dependencies are built without debuginfo on purpose.
- New level fields (`hazard`, `mover.one_shot`, `triggers`, `quakes`, `props`, `on_death`) default
  off. A trigger fires when the player *enters* its sector (so the start sector's triggers fire on
  the first tick); `once` defaults to true.
- Props collide with bodies but not with shots or pathfinding, and must not stand in a mover
  sector (`validate` enforces it).
- Enemy bolts and pellets hit any body in the way (friendly fire); a hurt enemy turns on an
  attacker of another kind. Bosses never do, and ignore their own splash.
- `on_death` runs when the actor is `Dead`, not at the kill. `rr-tools validate` counts triggers
  and `on_death` as channel sources and exits.
- `Automap` is indexed by wall: rebuild it whenever `CurrentMap` changes (`SpawnLevel` does).

## Git and pull requests

- Never push to `main`. Work on a feature branch and open a PR; PRs are squash-merged.
- Commit subjects follow Conventional Commits (`feat:`, `fix:`, `docs:`, `ci:`, `chore:`, …).
- Sign off every commit (DCO): `git commit -s`.
- GitHub Actions are pinned to full commit SHAs with a `# vX.Y.Z` comment; Dependabot keeps them
  current.

### Screenshots are required on every PR

Every PR description must include at least one screenshot that shows the change:

- **Gameplay, rendering, HUD or level changes:** an in-game screenshot of the result.
- **Non-visual changes** (CI, docs, pure `rr-core` logic): a screenshot of the evidence instead,
  e.g. the green workflow run, passing test output, or a `render-svg` map of a changed level.

Ways to capture in-game frames:

- Play (`cargo run -p rr-game --release`) and use your OS screenshot tool.
- Reproducible and headless-friendly: replay a demo script and record its frames, then pick one:

      cargo run -p rr-game --release -- arsenal_depot.ron \
          --demo assets/demo/arsenal_depot.ron --record /tmp/frames

  Write a short script of your own in `assets/demo/` (or a temp file) to frame the change.

Attach the images with `gh`'s `--attach` flag, which uploads them to
`github.com/user-attachments/assets` (the same pipeline as drag-and-drop in the web UI) and
rewrites any `![alt](./path.png)` reference in the body to the uploaded asset:

- New PR: `gh pr create --body-file body.md --attach './shot.png#Alt text'`
- Existing PR: `gh pr edit <number> --attach './shot.png#Alt text'` (repeat `--attach` per file)
- Comment: `gh pr comment <number> --attach './shot.png#Alt text'`

Do not commit screenshots to the repository.
