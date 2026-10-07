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
| `crates/tools` (`rr-tools`) | `validate`, `info`, `render-svg [--ids]` and `build [--check]` for level files. |
| `assets/episode.ron` | The episode: its name and the levels in play order. |
| `assets/fonts/` | The UI/HUD font (CC0 TTF). |
| `levels/src/*.ron` | Level sources for `rr-tools build`: sectors as `rect`/`poly` coordinates with ids, named materials, `stairs`; built into `assets/levels/` (never hand-edit a built level). |
| `assets/levels/*.ron` | Levels (authored doors are open; the game closes them). Built from `levels/src/` for the episode levels; the dev levels (`arsenal_depot`, `combat_arena`, `engine_lab`, …) are hand-written. |
| `assets/defs/*.ron` | Weapon and enemy stats; `models.ron` maps enemies, viewmodel weapons, pickups and gag props to models (scale, placement, clip names, muzzles). |
| `assets/models/` | glTF (`.glb`) for enemies, viewmodel weapons, pickups and gag props (`props/`; a prop kind without a model gets a code-built fallback): original models built from `scripts/models/`, the rest CC0. The boot, kick leg, pipe-bomb fist and switch panels stay code-built. |
| `assets/demo/*.ron` | Scripted demo runs (`--demo`): one route per episode level, checked by `crates/game/tests/episode_demos.rs`. |
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
    cargo run -p rr-tools -- build --check levels/src/*.ron
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
- A `models.ron` weapon may set `spin: Some((node: "Barrels", axis: (x, y, z)))`: that glTF node turns
  about its local `axis` (default +z) while the weapon fires (the chaingun's `Barrels`). `rr-tools validate` checks the node
  exists in the model and the axis is a finite, non-zero vector.
- The `weapons.ron` HUD label (`hud_name`, else `name`) must be 1-10 characters (checked when the
  defs load), so it never wraps the status bar.
- Low-res mode keeps 360 rows; its width follows the window aspect (`lowres.rs`) instead of a fixed 640.
- Esc on the death screen opens pause (`PausedFrom` remembers where to return). The quake rumble
  is a game-side loop like the mover hums; the core cue mapping no longer emits it.
- `chase` charges a seen player only when the straight line is passable (`steer::path_open`);
  otherwise it follows the route, and with no route it faces the player and strafes in place (so it
  keeps firing) instead of grinding on the opening.
- Demo segments may use `goto: Some((x, y))` (walk there; ends on arrival or at `secs`; `forward` and `turn_deg` are ignored while `goto` is set) and `face_deg` (absolute heading, short way). A `goto` that times out is counted (`DemoPlayback::goto_timeouts`); `episode_demos` fails a route with any.
- Level sources and level files reject unknown fields (`deny_unknown_fields`), so a typo fails the
  build instead of being dropped. `rr-tools build` labels errors with the source path and names
  sectors by source id (`sector 1 "lobby"`); it does not write output that fails to validate, and
  `--check` also fails on a built level (one carrying the "Built by `rr-tools build`" line) whose
  source is not among those given.
- Episode levels hold 30-45 enemies on Normal (barrels excluded); Easy is about 2/3 of that, Hard about 1.25x.
- `validate`'s "cannot be reached from the start" item warnings are expected only for crack-wall stashes and jetpack-only ledges. It never treats a Crack as passable, so no key may sit behind one.
- `Automap` is indexed by wall: rebuild it whenever `CurrentMap` changes (`SpawnLevel` does).
- Material names must be in `rr_core::map::KNOWN_MATERIALS` (each has a `texel()` arm); `validate` rejects others. Fill lights set `shadows: false`; keep ≤ 12 shadowed lights per level.

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
