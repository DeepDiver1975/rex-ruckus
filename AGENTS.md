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
| `crates/core` (`rr-core`) | Pure simulation: sector maps, collision, movement, mechanics, combat, actors; triggers, quakes, hazards (`hazard.rs`), gag props (`props.rs`), infighting, boss phases and special moves (`actors/special.rs`), automap (`automap.rs`), world snapshots (`snapshot.rs`: capture and apply of world state; a co-op client applies them and never ticks AI). **No Bevy.** Z-up, metres; headings in radians, 0 = east, CCW. |
| `crates/game` (`rr-game`, bin `rex-ruckus`) | Bevy front end: rendering, input, HUD, demo playback, `src/automap.rs` (Tab overlay), `src/partners.rs` (co-op partner visuals); `src/menu/` holds the menu, pause, options, controls and stats screens, `menu/multiplayer.rs` the co-op screens (Multiplayer, Join, Lobby, NetNotice) and `menu/text_field.rs` their name and address fields. Convert between core and Bevy coordinates **only** in `src/coords.rs`. |
| `crates/game/src/net/` | Co-op on the game side: `NetMode` and `apply_frame` (`mod.rs`), `host.rs` (seats, queued remote input, snapshots, relayed events and HUD lines, LAN beacons), `client.rs` (joining, receiving into the `Timeline`, `Load`, seating the players, `NetNotice`), `predict.rs` (the client's own player: sends its input, predicts it now, reconciles with the host's snapshot and eases the camera over a correction), `interp.rs` (`Timeline`: the rest of the world drawn `INTERP_DELAY` ticks behind the newest snapshot, blending between snapshots and releasing relayed events after their snapshot) and `lobby.rs` (the Join screen's `LanGames`). |
| `crates/net` (`rr-net`) | Co-op wire and socket layer: renet host and client (`session.rs`, `parse_addr`), LAN beacons (`lan.rs`: `BeaconSender`, `LanListener`), protocol constants and messages, postcard encode/decode (`decode_server` also rejects non-finite or out-of-bounds snapshots, a bad seat, an oversized roster and bad level names), `check_hello`, `ServerMsg` `Snapshot{you,snap}`/`Roster`/`Load`/`Events(TickEvents)`/`Hud`/`Play(PlayView)` (events, HUD lines and the host's screen are reliable and bounded by `MAX_EVENTS`/`MAX_HUD`), `ClientMsg::Input(Vec<InputFrame>)` (at most `INPUT_REDUNDANCY` frames, seqs strictly increasing), `valid_level_name`, `InputFrame::clamped`, `NETCODE_PROTOCOL_ID`, the LAN `Beacon`, `InputFrame` and `assets_hash`. Depends on `rr-core` and renet (no Bevy). |
| `crates/tools` (`rr-tools`) | `validate`, `info`, `render-svg [--ids]` and `build [--check]` for level files. |
| `assets/episode.ron` | The episodes in menu order, each with its name and levels in play order (no levels = "coming soon"). |
| `assets/fonts/` | The UI/HUD font (CC0 TTF). |
| `assets/icon/` | App icon sources (`rex-ruckus.svg`, `rex-ruckus-small.svg` for 16-32 px) and the built 256 px PNG. |
| `levels/src/*.ron` | Level sources for `rr-tools build`: sectors as `rect`/`poly` coordinates with ids, named materials, `stairs`; built into `assets/levels/` (never hand-edit a built level). |
| `assets/levels/*.ron` | Levels (authored doors are open; the game closes them). Built from `levels/src/` for the episode levels; the dev levels (`arsenal_depot`, `combat_arena`, `engine_lab`, …) are hand-written. |
| `assets/defs/*.ron` | Weapon and enemy stats; `models.ron` maps enemies, viewmodel weapons, pickups, gag props and the co-op player model to models (scale, placement, clip names, muzzles). |
| `assets/models/` | glTF (`.glb`) for enemies, viewmodel weapons, pickups, gag props (`props/`; a prop kind without a model gets a code-built fallback) and the co-op player Rex (`players/`): original models built from `scripts/models/`, the rest CC0. The boot, kick leg, pipe-bomb fist and switch panels stay code-built. |
| `assets/demo/*.ron` | Scripted demo runs (`--demo`): one route per episode level, checked by `crates/game/tests/episode_demos.rs`. |
| `assets/sounds/` | `bank.ron` (event to sound map), `synth.ron` recipes, `synth/` rendered output, `cc0/` curated CC0 recordings. |
| `assets/music/` | Level music (OGG). |
| `assets/quips/` | Hero voice quips (OGG) and `quips.ron` (texts). |
| `scripts/` | `gen-quips.sh` + `quips_kokoro.py` (Kokoro voice quips), `import-sfx.sh` (CC0 import via sox), `import-models.sh` (CC0 model import; uses `blend_to_glb.py`, a Blender conversion for models shipped only as `.blend`), `build-models.sh` + `models/` (original models as headless-Blender Python scripts, sharing `models/rrkit.py`), `record-demo.sh` (README demo), `build-icons.sh` (icon, `.ico` and installer wizard images from `assets/icon/*.svg`). |
| `installer/` | Inno Setup 6.6+ script for the Windows installer, `rex-ruckus.ico` and `wizard/` images (built). |

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

- Co-op partners are drawn as Rex (`models.ron` `player`): materials named `RexTeam*` are authored
  white and tinted per slot (`slot_tints`, exactly `MAX_PLAYERS` entries, index = PlayerId);
  `partner_clip` ranks Death > Crouch > moving > Pain > Attack > Idle because Rex's Attack and Pain
  clips are standing poses. `--partners N` (1-3) spawns standing stand-ins for testing.
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
- New level fields (`hazard`, `mover.one_shot`, `triggers`, `quakes`, `blasts`, `props`, `on_death`)
  default off. A trigger fires when the player *enters* its sector (so the start sector's triggers fire on
  the first tick); `once` defaults to true.
- Props collide with bodies but not with shots or pathfinding, and must not stand in a mover
  sector (`validate` enforces it). Every kind shares the use cooldown: arcade 3 s, jukebox
  `JUKEBOX_SECS` (the `jukebox_stinger` length, checked by an rr-tools test; the game ducks the
  music meanwhile), payphone 1.5 s. A payphone is wall-mounted: put its centre 0.15 m (+ a hair)
  off the wall, facing out. Its first call reports `Destruct::secrets_left`, later ones a dial tone.
- A slider (`mover: (kind: Slide(by: (x, y)), …)`) moves its whole sector by `by`. It must be an
  island: one loop, every wall a portal into one track sector that holds it as a hole, with
  ≥ 0.05 m clearance along the whole path. Bodies standing on it ride along; one beside or under
  it makes it reverse. Items, props, switches and the player start may not sit on it (they would
  not move); `validate` enforces all of this. It never changes the track's topology, so a
  sliding gate does not block reachability in `validate`.
- Enemy bolts and pellets hit any body in the way (friendly fire); a hurt enemy turns on an
  attacker of another kind. Bosses never do, and ignore their own splash.
- `on_death` runs when the actor is `Dead`, not at the kill. `rr-tools validate` counts triggers
  and `on_death` as channel sources and exits.
- A `models.ron` weapon may set `spin: Some((node: "Barrels", axis: (x, y, z)))`: that glTF node turns
  about its local `axis` (default +z) while the weapon fires (the chaingun's `Barrels`). `rr-tools validate` checks the node
  exists in the model and the axis is a finite, non-zero vector.
- Every gun has a magazine (`clip`) and `reload` time; the boot and pipe bombs do not. A new gun's
  pickup ammo loads its magazine first. Reload animations are keyframe tracks in
  `crates/game/src/reload_anim.rs` (gun pose, a glove and a code-built prop per gun), checked by a test
  that the hand and prop come into view at the default 75° FOV; a new gun needs a track there.
- The `weapons.ron` HUD label (`hud_name`, else `name`) must be 1-10 characters (checked when the
  defs load), so it never wraps the status bar.
- Low-res mode keeps 360 rows; its width follows the window aspect (`lowres.rs`) instead of a fixed 640.
- Esc on the death screen opens pause (`PausedFrom` remembers where to return). The quake rumble
  is a game-side loop like the mover hums; the core cue mapping no longer emits it.
- `chase` charges a seen player only when the straight line is passable (`steer::path_open`);
  otherwise it follows the route, and with no route it faces the player and strafes in place (so it
  keeps firing) instead of grinding on the opening.
- A gunner with `aim_time > 0` (`enemies.ron`; Grunt, Enforcer) stands in `AiState::Attack` to raise its
  gun before a burst and holds it `AIM_HOLD` after; the game holds the Attack clip's first frame (the aim
  pose) until `ActorFired`. Tests that need a shot on the first tick set `aim_time = 0`.
- Demo segments may use `hunt: true` (turn toward the nearest visible enemy and fire when on target) for
  fights, so a route survives changes in enemy timing better than a scripted sweep.
- Demo segments may use `goto: Some((x, y))` (walk there; ends on arrival or at `secs`; `forward` and `turn_deg` are ignored while `goto` is set) and `face_deg` (absolute heading, short way). A `goto` that times out is counted (`DemoPlayback::goto_timeouts`); `episode_demos` fails a route with any.
- Level sources and level files reject unknown fields (`deny_unknown_fields`), so a typo fails the
  build instead of being dropped. `rr-tools build` labels errors with the source path and names
  sectors by source id (`sector 1 "lobby"`); it does not write output that fails to validate, and
  `--check` also fails on a built level (one carrying the "Built by `rr-tools build`" line) whose
  source is not among those given.
- Episode levels hold 30-45 enemies on Normal (barrels excluded); Easy is about 2/3 of that, Hard about 1.25x.
- `validate`'s "cannot be reached from the start" item warnings are expected only for crack-wall stashes and jetpack-only ledges. It never counts on a barrel or rocket to open a Crack, so no key may sit behind one; a Crack that a scripted blast on a fired (reachable) channel reaches is passable.
- `blasts: [(channel, at: (x, y, z), radius, damage, delay)]` are scripted explosions: the first time the channel fires, each goes off once after `delay` through combat's blast queue (splash with falloff, cracks, glass, lights, `Explosion`), owned by `Shooter::World` (no self-damage discount, no infighting). `Mechanics::fire` hands them to `Combat::tick` via `take_blasts`. `validate` wants a channel source, `at` inside a sector between floor and ceiling, radius > 0, damage and delay >= 0.
- A co-op client follows the host's screen (`ServerMsg::Play`, sent when it changes and after a
  joiner's `Load`): death, stats and episode-end screens come from the host, only the host continues
  or restarts, and a host restart keeps a client's pause menu open.
- A co-op client applies the host's snapshots and never simulates: its sim systems run on
  `sim_ticks`, which is false for `NetMode::Client`. `world_running` keeps a net game running under
  the pause menu. The host's `LevelRuns` changing (any `restart_level`/`load_level`) is what sends
  `Load` to the clients. A client's shards, sounds and lights come only from the host's relayed
  events; from a snapshot's changes it only marks `DirtySectors` for re-meshing (`net/interp.rs`),
  except for the first snapshot after a `Load`: it also brings in what happened before (visuals for
  appended actors via `ActorSpawned`, lights broken earlier put out silently).
- Bump `rr_net::PROTOCOL_VERSION` whenever a wire type or any snapshot type (`rr_core::snapshot` and what it embeds) changes shape; a host refuses a client with another version.
- A co-op client runs two clocks: its own player is predicted now (`net/predict.rs`), everything
  else is drawn `INTERP_DELAY` (6) ticks behind the newest snapshot (`net/interp.rs`'s `Timeline`).
  Relayed events are released into `FxQueue` right after their tick's snapshot is applied, never
  on arrival, and the local player's relayed weapon events are dropped (prediction played them).
  Events and snapshots at or before the last `Load` tick are the old run's and are dropped.
- Every relayed event passes `rr_core::events::*_event_fits` against the client's world before
  any reader sees it; a new event that carries an index needs a check there.
- `Automap` is indexed by wall: rebuild it whenever `CurrentMap` changes (`SpawnLevel` does).
- The Hive Queen (`HiveQueen`) is the second boss. Her moves are data: a phase's `burrow` and
  `pound` and the def's `minions` (Slashers, live cap per skill via `Combat::skill`) in
  `enemies.ron`; `actors::special::step` runs them before `think`. She burrows only with ≥ 2
  `burrow_points` in the level (`validate` warns otherwise; points must be reachable, off movers
  and 1.4 m clear of walls). Underground (`AiState::Under`) an actor is alive but not
  `targetable()`: shots, blasts, pounds and collision skip it. Minions are appended to
  `Combat::actors` mid-level (`ActorSpawned`, also counted in the kill total); the game gives
  them visuals then. At most one boss of each kind per level.
- A hazard floor's material comes from its kind (`mark_hazard_floors` overrides the authored
  `floor_mat`): `Slime` → `slime`, `Electric` → `electric`, `Burn` → `magma`. A plain `magma` or
  `water` floor does not hurt.
- Material names must be in `rr_core::map::KNOWN_MATERIALS` (each has a `texel()` arm); `validate` rejects others. Fill lights set `shadows: false`; keep ≤ 12 shadowed lights per level.

- The app icon is built offline by `scripts/build-icons.sh` (rsvg-convert, icotool, ImageMagick 7)
  and its outputs are committed; edit the SVGs, never the PNG/ICO. The game embeds
  `assets/icon/rex-ruckus-256.png` as the window icon (`src/icon.rs`, X11/Windows; Wayland shows
  the `.desktop` icon matched by the `rex-ruckus` app id), and `crates/game/build.rs` embeds the
  `.ico` and version info in the Windows exe (only when building on Windows).

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
