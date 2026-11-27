{
  lib,
  callPackage,
  mkShell,
  rust-bin,
  git-cliff,
}:
let
  deps = callPackage ./deps.nix { };

  toolchain = rust-bin.stable.latest.default.override {
    extensions = [
      "rust-src"
      "rust-analyzer"
      "clippy"
      "rustfmt"
    ];
  };
in
mkShell {
  packages = [
    toolchain
    git-cliff
  ] ++ deps.nativeBuildInputs ++ deps.runtimeTools;

  buildInputs = deps.buildInputs;

  LD_LIBRARY_PATH = lib.makeLibraryPath deps.runtimeLibs;

  RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
}
