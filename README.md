# Rex Ruckus: Meltdown

A fast, interactive, 90s-style 3D shooter inspired by the Build-engine classics.
Original code and content; CC0 assets credited in `CREDITS.md`.

![Rex Ruckus demo: chaingun fight in the loading dock, a barrel chain reaction and a rocket through the atrium glass](docs/media/demo.gif)

▶ [Full-quality demo video (MP4)](docs/media/demo.mp4)

Run: `cargo run -p rr-game --release` (starts at the main menu)
Test: `cargo test --workspace`

## Download

Grab the latest build from the [latest release](https://github.com/DeepDiver1975/rex-ruckus/releases/latest).

- **Windows:** `rex-ruckus-<version>-windows-x64-setup.exe` installs the game with a Start Menu
  entry and an uninstaller.
- **Linux:** unpack `rex-ruckus-<version>-linux-x86_64.tar.gz` and run `./rex-ruckus.sh` from the
  unpacked folder (it needs ALSA and a Wayland or X11 session).

Every pull request and push to `main` also builds both; they are attached to the run of the
*Release builds* workflow as artifacts.

## Building

Requires Rust 1.95+ (edition 2024). On Linux, Bevy also needs the ALSA and udev
development packages, e.g. on Debian/Ubuntu:

    sudo apt install libasound2-dev libudev-dev pkg-config

To build the Windows installer locally, install [Inno Setup 6](https://jrsoftware.org/isinfo.php),
then:

    cargo build -p rr-game --release
    iscc /DAppVersion=0.1.0 installer\rex-ruckus.iss

The installer is written to `installer\Output\`. To build the Linux tarball locally, run
`scripts/package-linux.sh 0.1.0` after the release build; it lands in `dist/`. Pushing a `v*` tag
publishes the installer and the tarball as a GitHub release.

## License

GPL-3.0-or-later — see `LICENSE`.

## Menus, difficulty and settings

The game opens on the main menu. **New Game** asks for a difficulty (Easy, Normal or Hard) and then
plays the episode (`assets/episode.ron`) level by level, carrying health, armour, weapons and ammo
over; an end-of-level stats screen follows each level. **Esc** pauses (resume, options, restart level,
quit to menu). **Options** has mouse sensitivity, invert Y, field of view, the master, effects,
voice and music volumes, a *Low-res mode* (640x360) toggle for a chunky retro look, and **Controls**, where you pick an
action and press the new key or mouse button to rebind it (each action keeps up to two bindings,
Esc always opens the menu and cannot be rebound, Esc while capturing cancels).

`--difficulty easy|normal|hard` sets the difficulty for runs that skip the menu.

Settings (volumes, sensitivity, low-res, bindings) are saved to `settings.ron` in the OS config
directory:

- Linux: `~/.config/rex-ruckus/settings.ron` (or under `$XDG_CONFIG_HOME`)
- Windows: `%APPDATA%\rex-ruckus\config\settings.ron`
- macOS: `~/Library/Application Support/rex-ruckus/settings.ron`

Set `$RR_SETTINGS` to use another file. A missing file means defaults. Running a level directly
(`rex-ruckus LEVEL`) or a demo (`--demo`) skips the menu, uses default settings and never reads or
writes the settings file.

## Controls

Defaults (all rebindable under Options, except Esc). Click to capture the mouse (that first click does not fire) · Esc pauses and releases the mouse · WASD move · Mouse look · Space jump · C / Left Ctrl crouch · E use (doors, lifts, switches) · LMB fire · R reload · F quick-kick · 1–6 or the mouse wheel switch weapons (1 boot, 2 pistol, 3 shotgun, 4 chaingun, 5 rocket launcher, 6 pipe bombs; with bombs out, a fresh fire press detonates them) · Q use the medkit · J toggle the jetpack (Space climbs, C / Left Ctrl descends) · N toggle night vision · M mute / unmute · `[` and `]` lower / raise the master volume by 10%

## Audio

Sound effects are a mix of synthesised sounds (recipes in `assets/sounds/synth.ron`, rendered to
`assets/sounds/synth/`) and curated CC0 recordings (`assets/sounds/cc0/`); `assets/sounds/bank.ron`
maps game events to them. Rex's voice quips are generated with Kokoro-82M (voice `am_onyx`,
`assets/quips/`) and shown as a subtitle on their own HUD line. Each level can
name a music track (`assets/music/`). Pass `--mute` to start silent; `--record` is always silent
and skips music. The outputs are committed, so you only regenerate them after changing a recipe
or a quip:

    cargo run -p rr-tools -- synth assets/sounds/synth.ron -o assets/sounds/synth
    scripts/gen-quips.sh                       # needs Python 3.10-3.12, sox, espeak-ng
    scripts/import-sfx.sh SRC DEST             # imports a CC0 recording (sox)

`import-sfx.sh` honours `LOOP="START LEN"`, `CHANNELS=2` and `NO_TRIM=1`; see the script header.
`rr-tools quips-list` prints the quip texts that `gen-quips.sh` voices.

## Pickups, death and level complete

Walk over items to collect them: ammo (bullets, shells, rockets, pipe bombs), the shotgun, chaingun, rocket launcher, health packs, atomic health (up to 200), armour, the medkit, the jetpack and night-vision goggles. Keycards work as before. When you die or finish the level, press Use or Fire to restart.

## Levels

The default level is `arsenal_depot.ron`, the M4a showcase: rockets and exploding barrels, glass, drones, a jetpack shaft and a secret behind a crack wall a pipe bomb opens. `combat_arena.ron`, `mechanics_lab.ron` and `test_yard.ron` can be passed as an argument (a file name in `assets/levels/`, or a path to a level file):

    cargo run -p rr-game --release -- test_yard.ron

Add `--mute` to start with the sound off (`M` toggles it in game).

## Models

Enemies, viewmodel weapons and pickups are CC0 glTF models in `assets/models/`, mapped to the game
by `assets/defs/models.ron` and imported with `scripts/import-models.sh` (sources in `CREDITS.md`).
The boot, kick leg, pipe-bomb fist and switch panels are built from primitives at runtime.

## Tuning

Weapon and enemy stats live in `assets/defs/weapons.ron` and `assets/defs/enemies.ron`; edit them and restart.

## Level tools

    cargo run -p rr-tools -- validate assets/levels/*.ron
    cargo run -p rr-tools -- render-svg assets/levels/arsenal_depot.ron -o depot.svg

`validate` also checks that the sound bank covers every event, that quip files exist, that level music files exist and that models cover every enemy, weapon and pickup (clip and bone names, triangle budget) and that every third-party audio and model file is listed in `CREDITS.md`.

In the SVG, glass panes are dashed cyan, crack walls hatched and secret sectors starred.

## Property tests

CI runs the property tests with a small case count. A nightly workflow (`.github/workflows/nightly.yml`, also runnable by hand) reruns them at `PROPTEST_CASES=20000`. Set the variable yourself to go deeper locally:

    PROPTEST_CASES=2000 cargo test -p rr-core --release --test movement_props

## Demo playback and recording

`--demo` replays a scripted run instead of reading the keyboard and mouse (a RON list of timed
input segments; see `assets/demo/arsenal_depot.ron`), and `--record DIR` saves every frame as a
PNG at a fixed 30 fps:

    cargo run -p rr-game --release -- arsenal_depot.ron --demo assets/demo/arsenal_depot.ron --record /tmp/frames

`scripts/record-demo.sh` does that and encodes `docs/media/demo.mp4` and `docs/media/demo.gif`
(needs a display and ffmpeg). Rerun it after visible changes to keep the README demo current.
