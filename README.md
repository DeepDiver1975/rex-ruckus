# Rex Ruckus: Meltdown

A fast, interactive, 90s-style 3D shooter inspired by the Build-engine classics.
Original code and content; CC0 assets credited in `CREDITS.md`.

Run: `cargo run -p rr-game --release`
Test: `cargo test --workspace`

## Building

Requires Rust 1.95+ (edition 2024). On Linux, Bevy also needs the ALSA and udev
development packages, e.g. on Debian/Ubuntu:

    sudo apt install libasound2-dev libudev-dev pkg-config

## License

GPL-3.0-or-later — see `LICENSE`.

## Controls

Click to capture the mouse (that first click does not fire) · Esc releases it · WASD move · Mouse look · Space jump · C / Left Ctrl crouch · E use (doors, lifts, switches) · LMB fire · R reload · F quick-kick · 1–6 or the mouse wheel switch weapons (1 boot, 2 pistol, 3 shotgun, 4 chaingun, 5 rocket launcher, 6 pipe bombs; with bombs out, a fresh fire press detonates them) · Q use the medkit · J toggle the jetpack (Space climbs, C / Left Ctrl descends) · N toggle night vision

## Pickups, death and level complete

Walk over items to collect them: ammo (bullets, shells, rockets, pipe bombs), the shotgun, chaingun, rocket launcher, health packs, atomic health (up to 200), armour, the medkit, the jetpack and night-vision goggles. Keycards work as before. When you die or finish the level, press Use or Fire to restart.

## Levels

The default level is `arsenal_depot.ron`, the M4a showcase: rockets and exploding barrels, glass, drones, a jetpack shaft and a secret behind a crack wall a pipe bomb opens. `combat_arena.ron`, `mechanics_lab.ron` and `test_yard.ron` can be passed as an argument (a file name in `assets/levels/`, or a path to a level file):

    cargo run -p rr-game --release -- test_yard.ron

## Tuning

Weapon and enemy stats live in `assets/defs/weapons.ron` and `assets/defs/enemies.ron`; edit them and restart.

## Level tools

    cargo run -p rr-tools -- validate assets/levels/*.ron
    cargo run -p rr-tools -- render-svg assets/levels/arsenal_depot.ron -o depot.svg

In the SVG, glass panes are dashed cyan, crack walls hatched and secret sectors starred.

## Property tests

CI runs the property tests with a small case count. A nightly workflow (`.github/workflows/nightly.yml`, also runnable by hand) reruns them at `PROPTEST_CASES=20000`. Set the variable yourself to go deeper locally:

    PROPTEST_CASES=2000 cargo test -p rr-core --release --test movement_props
