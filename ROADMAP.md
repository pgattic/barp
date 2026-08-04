# BARP Roadmap

Rust (axum) backend, EmulatorJS frontend, flat-file storage, NixOS module deployment.

BARP = Boring Ahh ROM Player.

---

## Phase 0 — Setup
- [x] `cargo new barp`, add deps: axum, tokio, serde/serde_json, argon2, tower-http, rust-embed
- [x] flake.nix dev shell

## Phase 1 — Backend skeleton
- [x] Config struct: `roms_path`, `saves_path`, `port`, `default_options`, `system_mappings`, `users.<username>.{password_hash|password_hash_file, option_overrides}`
- [x] Load config JSON at startup
- [x] `/healthz`, logging via `tracing`

## Phase 2 — Auth & sessions
- [x] `POST /api/login` → issue session token
- [x] In-memory session map (no DB)
- [x] Session cookie: HttpOnly, SameSite=Lax
- [x] `POST /api/logout`
- [x] Sessions drop on restart — accepted behavior

## Phase 3 — Browsing & file serving
- [x] `GET /api/browse/*path` — list immediate children of a path (dirs + files), reflects filesystem as-is, any depth
  - response: `[{name, type: "dir" | "file"}]`
  - `path = ""` → top-level folders (systems)
  - unrecognized top-level folders are hidden from the browser (startup still warns)
- [x] System detection: `first_segment(path) -> EJS_core`, static lookup table in code (not Nix config), computed at launch time from a file's full path — not baked into browsing
- [x] `GET /api/roms/*path` — stream file bytes, range-request support
- [x] Path traversal guard shared by both routes (single sanitization helper)
- [x] Loud error on unrecognized first segment (no silent fallback)

## Phase 4 — Saves (flat-file only)
- [x] Layout: `saves/<username>/*path` mirrors the ROM path with the ROM extension replaced by `.srm` or `.stateN`
- [x] Atomic write (tmp + rename), per-user lock
- [x] `GET/PUT /api/saves/*path`
- [x] No per-user config.json — settings are NOT stored server-side (see Phase 5)

## Phase 5 — Frontend shell
- [x] Login screen → system list → game list → EmulatorJS player
- [x] `GET /api/bootstrap` returns merged `default_options + this user's option_overrides` (from Nix config only)
- [x] Display options applied from Nix/config on each play page (shader / smooth / integer scale); EmulatorJS keeps its own localStorage for in-player settings
- [x] Custom trimmed EmulatorJS chrome via `EJS_Buttons` (cache manager hidden; BARP owns save/load)
- [x] EmulatorJS save state / battery save calls wired to `/api/saves/...`
- [x] Build → embed BARP frontend via rust-embed; serve EmulatorJS from `emulatorjs_path`
- [x] Gamepad navigation in the ROM browser

## Phase 6 — Input
- [x] Mobile viewport / touch controls (100dvh, virtual gamepad auto-hide when a physical pad is used)
- [ ] Confirm multiple simultaneous gamepads map to player 1/2/3/4 (couch co-op) — expected to work out of the box via EmulatorJS + Gamepad API, verify on real controllers
- [ ] Keyboard fallback mapping

## Phase 7 — Hardening
- [x] Login rate limiting
- [x] Path sanitization (shared join/sanitize helpers + unit tests)
- [x] systemd sandboxing: DynamicUser, ProtectSystem=strict, ReadOnlyPaths (roms), ReadWritePaths (saves)
- [x] Fail loudly on missing/unreadable roms_path
- [x] Regenerate user's saves dir if deleted externally, don't crash (`create_dir_all` on write)

## Phase 8 — NixOS module
- [x] Package via flake `crane` (cached cargo deps + separate EmulatorJS release package)
- [x] `services.barp`: `enable`, `romsPath`, `savesPath`, `port`, `defaultOptions`, `users.<name>.{passwordHashFile, optionOverrides}`
- [x] Generate config JSON from module options
- [x] systemd unit with Phase 7 sandboxing
- [x] Document argon2 CLI / argon2.online + agenix workflow
- [x] Deployed: declare user → rebuild → login → saves persist

## Phase 9 — Rollout
- [x] Point at real library, validate folder-name assumptions
- [ ] Add household users
- [x] Verify mobile browser on real phone
- [ ] Verify 2+ controller couch co-op on real hardware
- [ ] README: backing up saves/ (folder conventions and adding users are partially documented)

---

## Known limitations
- Nintendo DS is not a built-in system (cart saves bind at load; EmulatorJS only exposes the path after boot)
- PSP is not a built-in system (EmulatorJS marks `ppsspp` as `save: false`; memory-stick saves are not a single `.srm`)
- Systems that need BIOS still require you to supply firmware EmulatorJS cannot ship

## Non-Goals
- Metadata scraping, box art, video previews
- Database of any kind — filesystem + config file only
- Admin UI for user management (config-declared only)
- Per-user server-stored settings (Config defaults + EmulatorJS localStorage)
- Netplay (remote multiplayer) — extra always-on service, NAT/TURN complexity, known-flaky upstream; use Remote Play Together / Moonlight-Sunshine instead if ever needed
- RetroAchievements / cloud sync beyond saves/ backups
