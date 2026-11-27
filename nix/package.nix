{
  lib,
  callPackage,
  rustPlatform,
  makeWrapper,
  patchelf,
}:
let
  deps = callPackage ./deps.nix { };
  workspaceVersion = (lib.importTOML ../Cargo.toml).workspace.package.version;
in
rustPlatform.buildRustPackage {
  pname = "prybar";
  version = workspaceVersion;

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [ "-p" "prybar" ];

  nativeBuildInputs = deps.nativeBuildInputs ++ [
    makeWrapper
    patchelf
  ];
  buildInputs = deps.buildInputs;

  dontUseCmakeConfigure = true;

  dontCargoUseSystemLibs = true;

  doCheck = false;

  postInstall = ''
    install -Dm644 crates/app/assets/desktop/prybar.desktop \
      $out/share/applications/prybar.desktop
    install -Dm644 crates/app/assets/desktop/prybar-mime.xml \
      $out/share/mime/packages/prybar.xml
    install -Dm644 crates/app/assets/desktop/prybar.svg \
      $out/share/icons/hicolor/scalable/apps/prybar.svg
  '';

  postFixup = ''
    patchelf --add-rpath ${lib.makeLibraryPath deps.runtimeLibs} $out/bin/prybar
    wrapProgram $out/bin/prybar \
      --prefix PATH : ${lib.makeBinPath deps.runtimeTools}
  '';

  passthru = { inherit deps; };

  meta = {
    description = "Compile, view and pack Source engine models and addons";
    homepage = "https://github.com/JonnyBro/prybar";
    license = lib.licenses.gpl3Plus;
    mainProgram = "prybar";
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
