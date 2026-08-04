# BARP

**BARP** (Boring Ahh ROM Player) is a small Axum service for browsing a ROM directory and launching games through EmulatorJS. It stores saves as flat files under each authenticated user.

## Development

Use the Nix dev shell:

```sh
nix develop --builders ''
cargo run -- --config config.example.json
```

`config.example.json` shows the expected config shape. Each user must set
exactly one of `password_hash` (inline Argon2 PHC string) or
`password_hash_file` (path to a file containing that string). Prefer the file
form for real deployments so hashes stay out of config and the Nix store.

## Command Line and Logs

BARP serves `config.json` by default. Use `--config` to select another file:

```sh
barp --config /etc/barp/config.json
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
              passwordHashFile = "/run/agenix/barp-player1-hash";
            };
          };
        }
      ];
    };
  };
}
```

Generate Argon2id PHC password hashes with the [`argon2`](https://github.com/P-H-C/phc-winner-argon2)
CLI or [argon2.online](https://argon2.online/). Use settings that match OWASP's
minimum recommendation:

| Option | Value |
|--------|-------|
| Variant | Argon2id |
| Memory cost | `19456` KiB (19 MiB) |
| Iterations | `2` |
| Parallelism | `1` |
| Hash length | `32` bytes |

With the CLI (`-k` is memory in KiB; if your build only has `-m`, use `-m 15` for 32 MiB instead):

```sh
echo -n 'your-password' | argon2 "$(openssl rand -base64 12)" -id -t 2 -k 19456 -p 1 -l 32 -e
```

On argon2.online, choose **Argon2id**, set the table values above, leave salt random,
and copy the **encoded** output (the `$argon2id$v=19$...` string). Prefer the CLI
for passwords you care about — a browser hash generator sees the plaintext.

Store the PHC string either inline as `password_hash` in `config.json`, or in a
file (agenix, sops-nix, or a root-readable path) and point `passwordHashFile` /
`password_hash_file` at it. The NixOS module only exposes the file option so
hashes do not land in the world-readable Nix store. The module generates BARP's
JSON config and runs a sandboxed systemd service with EmulatorJS provided by
`packages.emulatorjs`.

### Behind a Reverse Proxy

Save states are uploaded as whole request bodies, and they get large fast: a few
kilobytes for NES, several megabytes for mGBA, more for N64. BARP accepts
up to 64 MiB, so the proxy must not be stricter. With nginx:

```nix
services.nginx.clientMaxBodySize = "64m";
```

nginx's own default is 1 MB, and NixOS raises it only to 10 MB. Too low a limit
shows up as save states that silently never persist while small `.srm` battery
saves still work.
