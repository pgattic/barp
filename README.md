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

Browsing is plain filesystem browsing under `roms/`.

- `/browse/` shows the ROM root.
- `/browse/<path>` browses `roms/<path>`.
- `/play/<path>` opens `roms/<path>` in the player.

The player still uses the first path segment to select the EmulatorJS core, so the selected ROM must live under a recognized top-level folder.

Only recognized ROM extensions are shown by the browser.

## EmulatorJS Assets

The embedded frontend expects EmulatorJS's runtime assets under `frontend/emulatorjs/data/`, served as `/emulatorjs/data/`.

Vendor the pinned upstream release with:

```sh
nix develop --builders '' --command scripts/vendor-emulatorjs.sh
```
