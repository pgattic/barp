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

Only recognized ROM extensions are shown by the browser.

## EmulatorJS Assets

The embedded frontend expects EmulatorJS's runtime assets under `frontend/emulatorjs/data/`, served as `/emulatorjs/data/`.

Vendor the pinned upstream release with:

```sh
nix develop --builders '' --command scripts/vendor-emulatorjs.sh
```
