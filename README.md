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
- `prybar decompile <model> <output dir> [--appid N --game-dir D]`: writes a QC and SMD files: meshes, skeleton, materials, skins, bodygroups, attachments, hitboxes, collision, LODs, animations, sequences, flexes, eyes, jigglebones, procedural bones and bone options such as `$bonemerge` and `$limitrotation`. A game path needs `--appid` and `--game-dir`. A model can also be decompiled from inside a VPK or GMA with the Decompile To… button in Browse Packages and Game Files.
- `prybar package list|unpack <file> [output dir]`: reads VPK, GMA and old workshop `.bin` files.
- `prybar package pack <appid> <vpk|gma> <folder> <output> [--multi] [--game-tool] [-- options]`: packs without the game's tools.
  - `--multi`: split the VPK into chunks, `-- -c <MB>` sets their size;
  - `-- -warninvalid`: skip files that are not on `gmad`'s whitelist (GMA only);
  - `--game-tool`: use the game's `vpk` or `gmad` instead.
- `prybar package mounts <appid> <game dir>`: the game's search paths, first to last.
- `prybar package which <appid> <game dir> <path>`: every copy of a file the game has, the loaded one first. Garry's Mod follows the order on the Facepunch wiki, other games their `gameinfo.txt`.
- `prybar workshop list|publish <appid> ...`: the Garry's Mod workshop through `gmpublish`. The native Linux build of Garry's Mod ships `gmad_linux` and `gmpublish_linux`, so packing and publishing work without the Windows tools.
- `prybar download -o <output dir> <links or IDs...>`: workshop items and collections, from Steam's cache or their direct link.

## Compared with Crowbar

Decompile time on Garry's Mod models (loose files, Crowbar 0.74 under Wine):

| Model | Prybar | Crowbar |
| --- | --- | --- |
| advisor | 0.04 s | 0.10 s |
| alyx | 0.23 s | 0.56 s |
| antlion_guard | 0.32 s | 0.82 s |
| combine_soldier | 0.07 s | 0.18 s |
| combine_soldier_anims | 0.30 s | 1.28 s |
| police | 0.07 s | 0.21 s |
| police_animations | 0.40 s | 1.51 s |
| vortigaunt | 0.10 s | 0.34 s |
| **Total** | **1.5 s** | **5.4 s** |

Crowbar's times start at its first output file, so they leave out the time it spends reading the model.

| | Prybar | Crowbar |
| --- | --- | --- |
| Platform | native Linux | Windows .NET, Wine on Linux |
| MDL versions it decompiles | Source v44 to v49 | GoldSrc and Source up to v53 |
| Decompile a whole folder | no, one model at a time | yes |
| Decompile from inside a VPK or GMA | yes | no, unpack first |
| QC, SMD, LODs, collision, animations, IK, flexes, eyes | yes | yes |
| Jigglebones, `.vrd`, bone and attachment options | yes | yes |
| `$bonecontroller`, axisinterp bones | no | yes |
| Compile with studiomdl | yes, can recompile when files change | yes |
| Open in HLMV or Hammer, start the game | yes | yes |
| Pack VPK and GMA | own packer, no game tools needed | game's `vpk` and `gmad` |
| Unpack | VPK, GMA, old workshop `.bin` | VPK, GMA and a few others |
| Workshop publishing | Garry's Mod only, through `gmpublish` | several games |
| Workshop downloads | yes, collections too | yes |
| Model check (checksums, materials, textures) | yes | no |
| Search path order, every copy of a file | yes | no |
| VTF preview, MDL inspector | yes | no |
| Command line | everything | no |
| Proton for the game's tools | found automatically | no |

## Layout

- `crates/core`: Steam, games, packages, models and the game's tools, without a window.
- `crates/app`: the gpui window and the command line.

Interface strings are in `crates/*/locales/*.yml`.

## Releases

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests. A `vX.Y.Z` tag matching the version in `Cargo.toml` builds the Linux archive and publishes a release with git-cliff notes.

## License

GPL v3 or later; see [LICENSE](LICENSE).\
Written from scratch, shares no code with Crowbar.
