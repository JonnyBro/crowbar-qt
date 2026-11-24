# Prybar

Native Linux tool for Source engine modding, inspired by [Crowbar](https://github.com/ZeqMacaw/Crowbar).
Works with games installed natively and through Proton.

> Prybar is being rewritten in Rust with [gpui](https://www.gpui.rs/). This branch has the new skeleton
> only: the window opens and the command line is defined, but no tool works yet.
> The last C++/Qt version (crowbar-qt 1.2.1) lives on the `backup-qt` branch.

## Building

Requires Rust 1.95 or newer, clang and mold (see `.cargo/config.toml`), and the usual gpui dependencies:
fontconfig, freetype, wayland, xkbcommon, xcb and Vulkan headers.

```bash
cargo build --release
```

```bash
cargo test
```

Run `target/release/prybar` for the window, or `target/release/prybar --help` for the command-line modes.

## Layout

- `crates/core` (`prybar-core`): everything without a window: Steam and games, packages, the game's tools.
- `crates/app` (`prybar`): the gpui window and the command line.

Interface strings live in `crates/*/locales/*.yml` (English and Russian). The command-line modes always print English.

## Releases

Every push and pull request runs `.github/workflows/ci.yml` (format, clippy, tests). Pushing a tag `vX.Y.Z`
that matches the workspace version in `Cargo.toml` runs `.github/workflows/release.yml`, which builds the Linux
binary and publishes a GitHub release with notes written by git-cliff from the Conventional Commits since the
previous tag.

## License

Prybar is free software under the GNU General Public License v3 or later; see [LICENSE](LICENSE).

It is a separate program written from scratch and shares no code with [Crowbar](https://github.com/ZeqMacaw/Crowbar).
