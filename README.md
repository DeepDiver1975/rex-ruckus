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

Click to capture the mouse · Esc releases it · WASD move · Mouse look · Space jump · C / Left Ctrl crouch · E use (doors, lifts, switches)

Walking over a keycard picks it up.

Run a specific level (a file name in `assets/levels/`, or a path to a level file):

    cargo run -p rr-game --release -- test_yard.ron

## Level tools

    cargo run -p rr-tools -- validate assets/levels/*.ron
    cargo run -p rr-tools -- render-svg assets/levels/mechanics_lab.ron -o lab.svg
