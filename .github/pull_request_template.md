## Summary

<!-- What does this change and why? -->

## Screenshots

<!-- Required — see AGENTS.md. In-game screenshots for visual changes; for CI, docs or pure logic
     changes, a screenshot of the workflow run, test output or a render-svg map. -->

## Test plan

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo run -p rr-tools -- validate assets/levels/*.ron`
