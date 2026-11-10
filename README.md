# crowbar-qt

Native Linux tool for Source engine modding, modelled on [Crowbar](https://github.com/ZeqMacaw/Crowbar).
Works with games installed natively and through Proton.

## Building

Requires CMake, a C++20 compiler, Qt 6.6 or newer and liblzma (xz).

```bash
cmake -B build
cmake --build build -j
ctest --test-dir build
```

Run `build/crowbar-qt`, or `build/crowbar-qt --list-games` to print the detected Source games without opening a window.

`build/crowbar-qt --compile <appid> <game dir> <qc file> [output dir]` compiles a model from the command line.
With an output dir, the compiled files are moved there from the game's `models` folder.

`build/crowbar-qt --view <appid> <game dir> [mdl file]` opens HLMV, optionally with a model.

Packages:

- `build/crowbar-qt --list-package <file.vpk|file.gma>` lists the files in a package.
- `build/crowbar-qt --unpack <file.vpk|file.gma> <output dir>` unpacks it without needing the game's tools.
- `build/crowbar-qt --pack <appid> <vpk|gma> <folder> <output file> [--multi] [packer options]` packs a folder with the game's `vpk` or `gmad`.

Workshop (Garry's Mod, through the game's `gmpublish`):

- `build/crowbar-qt --workshop-list <appid>` lists your published items.
- `build/crowbar-qt --publish <appid> [--id N] [--addon file.gma] [--icon image] [--changes text]` creates an item, or updates item N.
  Any image works as the icon; it is converted to the 512x512 JPEG gmpublish requires.

Running:

- `build/crowbar-qt --run-game <appid> <game dir> [options]` starts the game through Steam with `-game`.
- `build/crowbar-qt --hammer <appid> <game dir>` opens Hammer for that game folder.

- `build/crowbar-qt --download <link|id> <output dir>` saves a workshop item, from Steam's cache if you are subscribed,
  otherwise from its direct link (old items only). The Download tab can also use `steamcmd` when it is installed.
