# Barecade

Barecade is a small Axum service for browsing a ROM directory and launching games through EmulatorJS. It stores saves as flat files under each authenticated user.

## Development

Use the Nix dev shell:

```sh
nix develop --builders ''
cargo run -- --config config.example.json
```

`config.example.json` shows the expected config shape. Password hash files must contain Argon2 PHC strings.

## ROM Folders

The first path segment selects the EmulatorJS core. This MVP recognizes:

- `nes`
- `snes`
- `gb`
- `gbc`
- `gba`
- `n64`

Only recognized ROM extensions are shown by the browser.

The frontend mirrors ROM-relative paths in the browser URL:

- `/` shows the system folders.
- `/nes` browses `roms/nes`.
- `/nes/game.nes` opens `roms/nes/game.nes` in the player.

## EmulatorJS Assets

The embedded frontend expects EmulatorJS's runtime assets under `frontend/emulatorjs/data/`, served as `/emulatorjs/data/`.

Vendor the pinned upstream release with:

```sh
nix develop --builders '' --command scripts/vendor-emulatorjs.sh
```
