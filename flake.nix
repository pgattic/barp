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
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;
          craneLib = crane.mkLib pkgs;

          # Dependency builds only need Cargo sources. Keep frontend/fixtures
          # out so editing UI/tests does not invalidate the dep cache.
          cargoSrc = craneLib.cleanCargoSource ./.;

          # Final package needs frontend/ for rust-embed and tests/fixtures for
          # unit tests that load EmulatorJS core metadata.
          src = lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
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
            pname = "barp";
            version = "0.1.0";
            strictDeps = true;
          };

          cargoArtifacts = craneLib.buildDepsOnly (
            commonArgs
            // {
              src = cargoSrc;
            }
          );

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
          barp-emulatorjs = emulatorjs;
          docker = barp-docker;
        }
      );

      overlays.default = final: prev: {
        barp = self.packages.${final.stdenv.hostPlatform.system}.barp;
        barp-emulatorjs = self.packages.${final.stdenv.hostPlatform.system}.emulatorjs;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
        };

      nixosModules.barp = self.nixosModules.default;

      checks = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;
          craneLib = crane.mkLib pkgs;
          cargoSrc = craneLib.cleanCargoSource ./.;
          src = lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
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
          cargoArtifacts = craneLib.buildDepsOnly {
            pname = "barp";
            version = "0.1.0";
            src = cargoSrc;
            strictDeps = true;
          };
        in
        {
          # Clippy needs frontend/ for rust-embed, but still reuses cargoArtifacts.
          clippy = craneLib.cargoClippy {
            pname = "barp";
            version = "0.1.0";
            inherit src cargoArtifacts;
            strictDeps = true;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          };
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
