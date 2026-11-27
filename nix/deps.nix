{
  lib,
  pkg-config,
  clang,
  mold,
  cmake,
  fontconfig,
  freetype,
  libxkbcommon,
  wayland,
  libxcb,
  libx11,
  vulkan-loader,
  libglvnd,
  xdg-utils,
  shared-mime-info,
  desktop-file-utils,
}:
{
  nativeBuildInputs = [
    pkg-config
    clang
    mold
    cmake
  ];

  buildInputs = [
    fontconfig
    freetype
    libxkbcommon
    wayland
    libxcb
    libx11
    vulkan-loader
  ];

  runtimeLibs = [
    vulkan-loader
    libglvnd
    wayland
    libxkbcommon
    libxcb
    libx11
    fontconfig
    freetype
  ];

  runtimeTools = [
    xdg-utils
    shared-mime-info
    desktop-file-utils
  ];
}
