# Boring Ahh ROM Player

BARP is a small web app for browsing a ROM library and playing games in the
browser via EmulatorJS. Saves are flat files per user. Fully stateless. No
database, no admin UI, no metadata scraping.

Deploy now with our easy-to-use Docker image or our dead-simple (docker-free)
NixOS module!

## Quick start

Three deployment options:

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

Top-level folders under `roms/` are consoles (`nes`, `snes`, `genesis`, …).
BARP picks the EmulatorJS core from that folder name (case-insensitive aliases
included). Nested folders are fine; open a ROM file to play.

Optional `system_mappings` in config rename folders or pin a specific core —
see `config.example.json`. Some systems still need BIOS files EmulatorJS cannot
ship. Nintendo DS and PSP are not built in (saves do not fit BARP’s model);
you can opt in via `system_mappings` if you accept that.

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

`config.example.json` is the reference for paths, display options
(`shader` / `smooth` / `integer_scale`), and users. Default config path is
`config.json`; override with `--config`.

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
