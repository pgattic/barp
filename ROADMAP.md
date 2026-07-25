# BARP Roadmap

Rust (axum) backend, EmulatorJS frontend, flat-file storage, NixOS module deployment.

BARP = Boring Ahh ROM Player.

---

## Phase 0 — Setup
- [ ] `cargo new barp`, add deps: axum, tokio, serde/serde_json, argon2, tower-http, rust-embed
- [ ] flake.nix dev shell

## Phase 1 — Backend skeleton
- [ ] Config struct: `roms_path`, `saves_path`, `port`, `default_options`, `system_mappings`, `users.<username>.{display_name, password_hash_file, option_overrides}`
- [ ] Load config JSON at startup
- [ ] `/healthz`, logging via `tracing`

## Phase 2 — Auth & sessions
- [ ] `POST /api/login` → issue session token
- [ ] In-memory session map (no DB)
- [ ] Session cookie: HttpOnly, SameSite=Lax
- [ ] `POST /api/logout`
- [ ] Session secret: generate on first boot, persist to StateDirectory
- [ ] Sessions drop on restart — accepted behavior

## Phase 3 — Browsing & file serving
- [ ] `GET /api/browse/*path` — list immediate children of a path (dirs + files), reflects filesystem as-is, any depth
  - response: `[{name, type: "dir" | "file"}]`
  - `path = ""` → top-level folders (systems)
- [ ] System detection: `first_segment(path) -> EJS_core`, static lookup table in code (not Nix config), computed at launch time from a file's full path — not baked into browsing
- [ ] `GET /api/roms/*path` — stream file bytes, range-request support
- [ ] Path traversal guard shared by both routes (single sanitization helper)
- [ ] Loud error on unrecognized first segment (no silent fallback)

## Phase 4 — Saves (flat-file only)
- [ ] Layout: `saves/<username>/*path` mirrors the ROM path with the ROM extension replaced by `.srm` or `.stateN`
- [ ] Atomic write (tmp + rename), per-user lock
- [ ] `GET/PUT /api/saves/*path`
- [ ] No per-user config.json — settings are NOT stored server-side (see Phase 5)

## Phase 5 — Frontend shell
- [ ] Login screen → system list → game list → EmulatorJS player
- [ ] `GET /api/bootstrap` returns merged `default_options + this user's option_overrides` (from Nix config only)
- [ ] On first page load, if no local settings key exists, seed localStorage from `/api/bootstrap`; after that, localStorage is authoritative and the UI never calls back to the server for settings
- [ ] Custom trimmed settings panel (few options) using `EJS_Buttons` to hide EmulatorJS's default menu items
- [ ] EmulatorJS save state calls wired to `/api/saves/...`
- [ ] Build → embed via rust-embed

## Phase 6 — Input
- [ ] Confirm touch controls work on mobile viewport
- [ ] Confirm multiple simultaneous gamepads map to player 1/2/3/4 (couch co-op) — expected to work out of the box via EmulatorJS + Gamepad API, verify on real controllers
- [ ] Keyboard fallback mapping

## Phase 7 — Hardening
- [ ] Login rate limiting
- [ ] Path sanitization audit
- [ ] systemd sandboxing: DynamicUser, ProtectSystem=strict, ReadOnlyPaths (roms), ReadWritePaths (saves)
- [ ] Fail loudly on missing/unreadable roms_path
- [ ] Regenerate user's saves dir if deleted externally, don't crash

## Phase 8 — NixOS module
- [ ] Package via crane/naersk
- [ ] `services.barp`: `enable`, `romsPath`, `savesPath`, `port`, `defaultOptions`, `users.<name>.{passwordHashFile, displayName, optionOverrides}`
- [ ] Generate config JSON from module options
- [ ] systemd unit with Phase 7 sandboxing
- [ ] Document mkpasswd + agenix workflow
- [ ] Test: declare user → rebuild → login → saves persist across second rebuild

## Phase 9 — Rollout
- [ ] Point at real library, validate folder-name assumptions
- [ ] Add household users
- [ ] Verify mobile browser on real phone
- [ ] Verify 2+ controller couch co-op on real hardware
- [ ] README: folder conventions, adding users, backing up saves/

---

## Non-Goals
- Metadata scraping, box art, video previews
- Database of any kind — filesystem + Nix config only
- Admin UI for user management (Nix-declared only)
- Per-user server-stored settings (localStorage only, seeded from Nix defaults)
- Netplay (remote multiplayer) — extra always-on service, NAT/TURN complexity, known-flaky upstream; use Remote Play Together / Moonlight-Sunshine instead if ever needed
- RetroAchievements / cloud sync beyond saves/ backups

