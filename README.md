# Prybar

Native Linux tool for Source engine modding, inspired by [Crowbar](https://github.com/ZeqMacaw/Crowbar).
Works with games installed natively and through Proton.

> Prybar is being rewritten in Rust with [gpui](https://www.gpui.rs/). The core and the command line are
> ported; the window is still a skeleton.
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

## Command line

The command-line modes use the same games, chosen tools folders and Proton as the window, and print English.
Ctrl+C stops a running tool together with everything it started under Proton.

- `prybar games` prints the detected Source games, libraries and Proton builds.
- `prybar compile <appid> <game dir> <qc file> [output dir]` compiles a model; with an output dir, the
  compiled files are moved there from the game's `models` folder.
- `prybar view <appid> <game dir> [mdl file]` opens HLMV, `prybar hammer <appid> <game dir>` opens Hammer,
  and `prybar run <appid> <game dir> [launch options]` starts the game through Steam with `-game`.
- `prybar package list <file>` and `prybar package unpack <file> <output dir>` read VPK and GMA packages
  (old LZMA-compressed workshop `.bin` files too) without the game's tools.
- `prybar package pack <appid> <vpk|gma> <folder> <output file> [--multi] [-- packer options]` packs a folder
  with the game's `vpk` or `gmad`.
- `prybar workshop list <appid>` and `prybar workshop publish <appid> [--id N] [--addon file.gma] [--icon image] [--changes text]`
  work with the Garry's Mod workshop through `gmpublish`; any image works as the icon.
- `prybar download -o <output dir> <links or IDs...>` saves workshop items and whole collections, from Steam's
  cache if you are subscribed, otherwise from their direct link (old items only).

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
