{
  description = "BARP — Boring Ahh ROM Player";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          emulatorjs = pkgs.callPackage ./nix/emulatorjs.nix { };
          barp = pkgs.rustPlatform.buildRustPackage {
            pname = "barp";
            version = "0.1.0";
            src = pkgs.lib.cleanSourceWith {
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
            cargoLock.lockFile = ./Cargo.lock;
            meta = {
              description = "Boring Ahh ROM Player";
              mainProgram = "barp";
            };
          };
        in
        {
          inherit barp emulatorjs;
          default = barp;
          barp-emulatorjs = emulatorjs;
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

      devShells = forAllSystems (pkgs: {
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
      });
    };
}
