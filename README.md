# Boring Ahh ROM Player

BARP is a self-hosted ROM player for classic cartridge and handheld systems.
Fully stateless. No database, no metadata scraping. Just point it at a folder!

## Features

- Starts up in 0.0038 seconds, uses 5 MiB of ram
- Treats ROMs folder as read-only
- Stores save files and savestates in user-specific folders for easy backup
- No external state or database, just ROMs and saves/savestate files
- Simple, straightforward setup for Docker and NixOS
- Reads users and settings from a config file (no imperative configuration)

## Supported Platforms

Single-file cartridge / handheld ROMs. Top-level folder under `roms/` (aliases
in parentheses):

- **Nintendo**: `nes` (famicom), `snes`, `n64`, `gb`/`gbc`, `gba`, `vb`
- **Sega**: `sms`, `genesis`/`megadrive`, `gg`, `32x`
- **Atari**: `atari2600`, `atari7800`
- **Other**: `pce` (tg16 HuCards), `ngp`, `ws`

Systems that need BIOS files, multi-track CD images, or arcade ROM sets (PSX,
Saturn, Sega CD, 3DO, Coleco, Lynx, Amiga, MAME/FBNeo, DOS, VICE, …) are not
built in. You can still point a folder at any EmulatorJS core via
`system_mappings` if you want to experiment.

## Quick start

**Docker / Compose** (see `deploy/docker/`):

```sh
docker pull ghcr.io/pgattic/barp:latest
cd deploy/docker
cp config.example.json config.json   # add a real password hash (below)
mkdir -p roms saves                  # put system folders under roms/, e.g. roms/nes/
docker compose up -d
```

**NixOS** — import the flake module, then:

```nix
services.barp = {
  enable = true;
  openFirewall = true;
  romsPath = "/var/lib/roms";
  users.player1.passwordHashFile = "/run/agenix/barp-player1-hash";
};
```

**From source** (Cargo):

```sh
cp config.example.json config.json
# Download EmulatorJS 4.2.3 and point emulatorjs_path to it in config.json
cargo run -- --config config.json
```

Go to `http://localhost:3000` to play.

## Library layout

Top-level folders under `roms/` are consoles (see Supported Platforms above).
BARP picks the EmulatorJS core from that folder name (case-insensitive). Nested
folders are fine; open a ROM file to play.

Optional `system_mappings` in config rename folders or pin a specific core —
see `config.example.json`. Some systems still need BIOS files EmulatorJS cannot
ship.

## Users and passwords

Each user needs an **Argon2id** PHC hash, either inline as `password_hash` or in
a file via `password_hash_file`. On NixOS, prefer `passwordHashFile` +
`agenix`/`sops-nix` so hashes stay out of the world-readable Nix store.

```sh
# Generate the password hash to store
echo -n 'your-password' | argon2 "$(openssl rand -base64 12)" -id -t 2 -k 19456 -p 1 -l 32 -e
```

Settings: Argon2id, memory `19456` KiB, iterations `2`, parallelism `1`, hash
length `32`. If your `argon2` build has no `-k`, use `-m 15` (32 MiB) instead.
You can also use [argon2.online](https://argon2.online/) to generate hashes.

## Config and ops

Default config path is `config.json`; override with `--config`. Same shape as
`config.example.json`:

```json
{
  "roms_path": "./roms",
  "saves_path": "./saves",
  "emulatorjs_path": "./emulatorjs",
  "port": 3000,
  "default_options": {
    "shader": "disabled",
    "smooth": false,
    "integer_scale": false,
    "four_score": false
  },
  "system_mappings": {
    "fds": "nes",
    "homebrew-nes": "fceumm"
  },
  "users": {
    "player1": {
      "password_hash": "$argon2id$v=19$m=19456,t=2,p=1$..."
    },
    "player2": {
      "password_hash_file": "./secrets/player2.hash",
      "option_overrides": {
        "shader": "crt-mattias.glslp",
        "four_score": true
      }
    }
  }
}
```

| Field | Notes |
| --- | --- |
| `roms_path` / `saves_path` | Required. Absolute or relative paths. |
| `emulatorjs_path` | Required. EmulatorJS `data/` dir (must contain `loader.js`). The Docker image and NixOS module take care of this field for you. |
| `port` | Optional; default `3000`. |
| `default_options` | Optional. `shader` is an EmulatorJS shader name or `"disabled"`; `smooth` / `integer_scale` control upscaling; `four_score` forces NES players 3–4 on (ignored elsewhere). |
| `system_mappings` | Optional. Map a roms folder name to a builtin system target (`nes`, `snes`, …) or a concrete EmulatorJS core (`fceumm`, `pcsx_rearmed`, …). |
| `users` | Required; at least one. Each user needs exactly one of `password_hash` or `password_hash_file`. `option_overrides` merges over `default_options`. |

- **Docker:** mount `/config/config.json`, `/roms`, `/saves`. EmulatorJS is
  built-in. Image user is UID `1000`.
- **Logs:** stderr (systemd journal on NixOS). `RUST_LOG=barp=debug` for more.
- **Reverse proxy:** allow large bodies (save states up to 64 MiB). nginx:
  `client_max_body_size 64m;` (or NixOS `services.nginx.clientMaxBodySize = "64m"`).

Build the image yourself with `nix build .#barp-docker` then `docker load < result`.

## AI Usage Disclaimer

This project was made with help from artificial intelligence. However, all major
decisions were made, and all code output was reviewed, by a human who has
real-world experience with the technologies involved.

## Thanks

- The [EmulatorJS](https://emulatorjs.org/) project, for making this all possible
- Retroarch/libretro, for providing easy-to-use emulator cores
- [RomM](https://romm.app/), [Gaseous](https://github.com/gaseous-project/gaseous-server),
  [Retrom](https://github.com/jmberesford/retrom), etc. for giving me inspiration
- [copyparty](https://github.com/9001/copyparty) inspired me to make a solution
  that was just config-based
