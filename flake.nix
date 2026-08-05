{
  description = "BARP — Boring Ahh ROM Player";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
      cargoPackage = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
      mkBuildContext =
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;
          cargoSrc = craneLib.cleanCargoSource ./.;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: _type:
              let
                base = baseNameOf path;
              in
              !(builtins.elem base [
                "target"
                "result"
                "saves"
                "roms"
                "secrets"
                ".direnv"
              ]);
          };
          commonArgs = {
            pname = cargoPackage.name;
            version = cargoPackage.version;
            strictDeps = true;
          };
          cargoArtifacts = craneLib.buildDepsOnly (
            commonArgs
            // {
              src = cargoSrc;
            }
          );
        in
        {
          inherit
            pkgs
            craneLib
            cargoSrc
            src
            commonArgs
            cargoArtifacts
            ;
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          inherit (mkBuildContext system)
            pkgs
            craneLib
            src
            commonArgs
            cargoArtifacts
            ;

          barp = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              inherit src;
              meta = {
                description = "Boring Ahh ROM Player";
                mainProgram = "barp";
              };
            }
          );

          emulatorjs = pkgs.callPackage ./nix/emulatorjs.nix { };

          barp-docker = pkgs.callPackage ./nix/docker.nix {
            inherit barp emulatorjs;
          };
        in
        {
          inherit barp emulatorjs barp-docker;
          default = barp;
        }
      );

      overlays.default = final: _prev: {
        barp = self.packages.${final.stdenv.hostPlatform.system}.barp;
        barp-emulatorjs = self.packages.${final.stdenv.hostPlatform.system}.emulatorjs;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
        };
      checks = forAllSystems (
        system:
        let
          inherit (mkBuildContext system)
            craneLib
            cargoSrc
            src
            commonArgs
            cargoArtifacts
            ;
        in
        {
          # Clippy needs frontend/ for rust-embed, but still reuses cargoArtifacts.
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit src cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );
          fmt = craneLib.cargoFmt {
            src = cargoSrc;
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.curl
              pkgs.p7zip
              pkgs.python3
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.unzip
            ];
          };
        }
      );
    };
}
