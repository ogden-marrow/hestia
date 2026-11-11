# Contributing to Hestia

Thanks for helping. Small, focused pull requests are easiest to review.

## Setup

```bash
git clone <this repository> && cd hestia
cargo build --release
cargo test --release
```

You need a recent stable Rust toolchain (edition 2024) and, for the app, the system libraries Rune
needs (on Ubuntu: `libxkbcommon-dev libwayland-dev libvulkan-dev libfontconfig1-dev pkg-config`).

## Layout

- `lifestream/` engine and CLI, no UI; most logic and most tests live here
- `ui/` the Hestia desktop app (Rune); UI is headless-testable, add a test with each pane you change
- `tray/` the menu bar icon
- `packaging/` GNOME extension, systemd units, desktop entry; `install.sh` installs them

## Principles

- **Lean is law.** No legacy or compatibility code: replace the old thing with the new one.
- **One canonical way** to do each thing. If an action already has a home, extend it; do not add a second.
- **Never touch the user's files** beyond what a feature states. Lifestream is read-only on scanned folders.
- **Private by default.** Nothing leaves the machine; no network except loopback model hosts.
- No personal data, paths or hostnames in code, tests or fixtures.

## Before you open a pull request

- `cargo fmt` is not enforced yet; match the surrounding style.
- `cargo build --release` and `cargo test --release` pass.
- Describe what changed and why; link the issue.

By contributing you agree your work is licensed under the MIT License.
