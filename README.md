# BARP

**BARP** (Boring Ahh ROM Player) is a small Axum service for browsing a ROM directory and launching games through EmulatorJS. It stores saves as flat files under each authenticated user.

## Development

Use the Nix dev shell:

```sh
nix develop --builders ''
cargo run -- --config config.example.json
```

`config.example.json` shows the expected config shape. Password hash files must contain Argon2 PHC strings.

## Command Line and Logs

BARP serves `config.json` by default. Use `--config` to select another file:

```sh
barp --config /etc/barp/config.json
barp hash-password             # read the password from stdin
barp --help
```

Startup validates the config, ROM and EmulatorJS directories, password hashes,
and save-directory writability before listening. Successful startup logs the
resolved paths and configuration summary. Login attempts, completed save
writes, request failures, shutdown, and startup errors are also logged to
stderr, which systemd sends to the journal:

```sh
journalctl -u barp -f
```

The default level is `info`. Set `RUST_LOG` to adjust it, for example
`RUST_LOG=barp=debug,tower_http=debug` to include every HTTP request.

## ROM Browsing

Browsing follows the filesystem under `roms/`. A URL such as `/nes/` renders
the matching directory, while `/nes/Super Mario Bros.nes` opens that ROM in
the player. The server determines which page to render from the target's file
type.

The player still uses the first path segment to select the EmulatorJS core, so the selected ROM must live under a recognized top-level folder.

BARP includes common folder aliases for every system exposed by the
vendored EmulatorJS release, such as `nes`, `famicom`, `snes`, `genesis`,
`megadrive`, `psx`, `playstation`, `arcade`, `c64`, and `dos`. Matching is
case-insensitive. Files are filtered using the extensions supported by that
system's EmulatorJS core rather than one global extension list.

Additional aliases or concrete core selections can be declared in the config:

```json
"system_mappings": {
  "my-ps1-games": "psx",
  "accurate-ps1": "mednafen_psx_hw"
}
```

The object maps a top-level ROM folder to either an EmulatorJS system name or
a concrete core name from `$emulatorjs_path/cores/cores.json`.
Configured mappings override built-in aliases with the same folder name.

Some systems require firmware that EmulatorJS cannot distribute. Recognizing
those systems does not remove their upstream BIOS requirement.

## Display Options

`default_options` (and per-user `option_overrides`) control presentation:

```json
"default_options": {
  "shader": "disabled",
  "smooth": false,
  "integer_scale": true
}
```

- `shader` is the EmulatorJS shader setting. Use `"disabled"` or a built-in
  name such as `"crt-mattias.glslp"`, `"2xScaleHQ.glslp"`, or `"bicubic"`.
- `smooth` controls browser upscaling of the canvas (`false` = crisp pixels,
  `true` = bilinear). EmulatorJS itself hardcodes RetroArch `video_smooth`
  off; this option affects how the page scales that bitmap.
- `integer_scale` turns on RetroArch `video_scale_integer` and sizes the
  canvas to an integer multiple of the core framebuffer.
- The player forces RetroArch `aspect_ratio_index` to 1:1 PAR (square pixels).
  Without that, CRT-era cores (NES, SNES, Genesis, …) often default to a
  non-square pixel aspect and look stretched; handhelds like GBC already use
  1:1.

## EmulatorJS Assets

`emulatorjs_path` must point at an EmulatorJS `data/` directory (the folder
that contains `loader.js`). BARP serves that tree at `/emulatorjs/data/`.

For local development, unpack or clone EmulatorJS and point config at its
`data/` directory, for example:

```json
"emulatorjs_path": "/home/pgattic/git/emulatorjs/data"
```

Or vendor the pinned upstream release into the repo workspace:

```sh
nix develop --builders '' --command scripts/vendor-emulatorjs.sh
```

```json
"emulatorjs_path": "./frontend/emulatorjs/data"
```

## Packaging

The flake builds BARP with [crane](https://crane.dev). Cargo dependencies are
built once into a reusable `cargoArtifacts` derivation, so source-only changes
(including frontend assets) do not recompile the whole crate graph.

## NixOS Module

Import the flake module and declare users, ROM/save paths, and password hash
files:

```nix
{
  inputs.barp.url = "github:pgattic/barp";

  outputs = { nixpkgs, barp, ... }: {
    nixosConfigurations.arcade = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        barp.nixosModules.default
        {
          services.barp = {
            enable = true;
            openFirewall = true;
            romsPath = "/var/lib/roms";
            users.player1 = {
              displayName = "Player 1";
              passwordHashFile = "/run/agenix/barp-player1-hash";
            };
          };
        }
      ];
    };
  };
}
```

Generate password hashes with:

```sh
nix run .#barp -- hash-password
```

Store the PHC string in a file (agenix, sops-nix, or a root-readable path) and
point `passwordHashFile` at it. The module generates BARP's JSON config and
runs a sandboxed systemd service with EmulatorJS provided by
`packages.emulatorjs`.
