{
  description = "Prybar: compile, view and pack Source engine models and addons";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" ];

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
    in
    {
      packages = forAllSystems (pkgs: rec {
        prybar = pkgs.callPackage ./nix/package.nix {
          rustPlatform =
            let
              toolchain = pkgs.rust-bin.stable.latest.minimal;
            in
            pkgs.makeRustPlatform {
              cargo = toolchain;
              rustc = toolchain;
            };
        };
        default = prybar;
      });

      apps = forAllSystems (pkgs: rec {
        prybar = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.prybar}/bin/prybar";
        };
        default = prybar;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.callPackage ./nix/dev-shell.nix { };
      });

      overlays.default = final: prev: {
        prybar = self.packages.${final.stdenv.hostPlatform.system}.prybar;
      };

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
