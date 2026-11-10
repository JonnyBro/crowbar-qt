# crowbar-qt

Native Linux tool for Source engine modding, modelled on [Crowbar](https://github.com/ZeqMacaw/Crowbar).
Works with games installed natively and through Proton.

## Building

Requires CMake, a C++20 compiler and Qt 6.6 or newer.

```bash
cmake -B build
cmake --build build -j
ctest --test-dir build
```

Run `build/crowbar-qt`, or `build/crowbar-qt --list-games` to print the detected Source games without opening a window.

`build/crowbar-qt --compile <appid> <game dir> <qc file>` compiles a model from the command line.
