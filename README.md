# Prybar

Native Linux tool for Source engine modding, inspired by [Crowbar](https://github.com/ZeqMacaw/Crowbar). Works with native games and with games under Proton.

> Written in Rust with [gpui](https://www.gpui.rs/).\
> The last C++/Qt version (crowbar-qt 1.2.1) is on the `backup-qt` branch.

## Installing

Unpack `prybar-vX.Y.Z-linux-x86_64.tar.gz` from the releases page into a folder you own, such as `~/Apps/prybar`. Settings > Application > Add to Menu adds Prybar to the application menu and opens `.qc`, `.mdl`, `.vpk` and `.gma` files from the file manager in Prybar.

Prybar checks GitHub for updates at start (this can be turned off in the settings).\
If an update is available, the sidebar shows a button that downloads the release, verifies its checksum and replaces the binary.

## Building

Needs Rust 1.95+, clang, mold and the gpui dependencies: fontconfig, freetype, wayland, xkbcommon, xcb and Vulkan headers.

```bash
cargo build --release
```

```bash
cargo test
```

## Command line

Uses the same games, tools folders and Proton as the GUI. You can use Ctrl+C to stop any tool.

- `prybar games`: detected games, libraries and Proton builds.
- `prybar compile <appid> <game dir> <qc> [output dir]`: compiles a model, then moves it to the output dir.
- `prybar view <appid> <game dir> [mdl]`, `prybar hammer <appid> <game dir>`: open HLMV or Hammer.
- `prybar run <appid> <game dir> [options]`: starts the game through Steam with `-game`.
- `prybar inspect <appid> <game dir> <model>`: checks a model's `.vvd`/`.vtx`/`.phy` checksums, materials, textures and `$includemodel` files the way the game finds them. The model is a file or a game path such as `models/props_c17/oildrum001.mdl`.
- `prybar decompile <model> <output dir> [--appid N --game-dir D]`: writes a QC and SMD files: meshes, skeleton, materials, skins, bodygroups, attachments, hitboxes, collision, LODs, animations, sequences, flexes and eyes. A game path needs `--appid` and `--game-dir`.
- `prybar package list|unpack <file> [output dir]`: reads VPK, GMA and old workshop `.bin` files.
- `prybar package pack <appid> <vpk|gma> <folder> <output> [--multi] [--game-tool] [-- options]`: packs without the game's tools.
  - `--multi`: split the VPK into chunks, `-- -c <MB>` sets their size;
  - `-- -warninvalid`: skip files that are not on `gmad`'s whitelist (GMA only);
  - `--game-tool`: use the game's `vpk` or `gmad` instead.
- `prybar package mounts <appid> <game dir>`: the game's search paths, first to last.
- `prybar package which <appid> <game dir> <path>`: every copy of a file the game has, the loaded one first. Garry's Mod follows the order on the Facepunch wiki, other games their `gameinfo.txt`.
- `prybar workshop list|publish <appid> ...`: the Garry's Mod workshop through `gmpublish`.
- `prybar download -o <output dir> <links or IDs...>`: workshop items and collections, from Steam's cache or their direct link.

## Layout

- `crates/core`: Steam, games, packages, models and the game's tools, without a window.
- `crates/app`: the gpui window and the command line.

Interface strings are in `crates/*/locales/*.yml`.

## Releases

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests. A `vX.Y.Z` tag matching the version in `Cargo.toml` builds the Linux archive and publishes a release with git-cliff notes.

## License

GPL v3 or later; see [LICENSE](LICENSE).\
Written from scratch, shares no code with Crowbar.
