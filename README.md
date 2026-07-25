# Barecade

Barecade is a small Axum service for browsing a ROM directory and launching games through EmulatorJS. It stores saves as flat files under each authenticated user.

## Development

Use the Nix dev shell:

```sh
nix develop --builders ''
cargo run -- --config config.example.json
```

`config.example.json` shows the expected config shape. Password hash files must contain Argon2 PHC strings.

## ROM Browsing

Browsing follows the filesystem under `roms/`. A URL such as `/nes/` renders
the matching directory, while `/nes/Super Mario Bros.nes` opens that ROM in
the player. The server determines which page to render from the target's file
type.

The player still uses the first path segment to select the EmulatorJS core, so the selected ROM must live under a recognized top-level folder.

Barecade includes common folder aliases for every system exposed by the
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
a concrete core name from `frontend/emulatorjs/data/cores/cores.json`.
Configured mappings override built-in aliases with the same folder name.

Some systems require firmware that EmulatorJS cannot distribute. Recognizing
those systems does not remove their upstream BIOS requirement.

## EmulatorJS Assets

The embedded frontend expects EmulatorJS's runtime assets under `frontend/emulatorjs/data/`, served as `/emulatorjs/data/`.

Vendor the pinned upstream release with:

```sh
nix develop --builders '' --command scripts/vendor-emulatorjs.sh
```
