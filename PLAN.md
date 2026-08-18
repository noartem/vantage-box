# Vantage Box — development plan

> A minimal desktop GUI for sing-box: takes your existing `config.json` and drives the runtime through the Clash API (`experimental.clash_api`, `127.0.0.1:9090`). No config format of its own, no magic.

## Stack

- **Tauri 2** (Rust backend + webview frontend) — binary ~5–10 MB, minimal memory, official plugins for everything needed: `tauri-plugin-global-shortcut`, tray API, `tauri-plugin-autostart`, `tauri-plugin-single-instance`, `tauri-plugin-updater`.
- **Frontend**: Svelte + TypeScript + Vite (minimal runtime; React/Vue are fine too, but Svelte is lighter). Styles — plain CSS or UnoCSS.
- **Rust crates**: `tokio` (async), `reqwest` (HTTP to the Clash API), `tokio-tungstenite` (WebSocket for `/traffic`, `/logs`, `/connections`), `serde_json`, `notify` (settings file watcher).

## Architecture

```
┌─────────────────────────────────────────┐
│ Tauri app (user-level, no admin)        │
│  ├─ UI (webview): dashboard, logs,      │
│  │   selectors, config editor           │
│  ├─ Rust core:                          │
│  │   ├─ ClashApiClient (HTTP+WS 9090)   │
│  │   ├─ ServiceController (start/stop)  │
│  │   ├─ Settings (settings.json+watch)  │
│  │   └─ Hotkeys, Tray                   │
└──────────────┬──────────────────────────┘
               │ process control
┌──────────────▼──────────────────────────┐
│ sing-box (system service, elevated)    │
│  └─ Clash API on 127.0.0.1:9090         │
└─────────────────────────────────────────┘
```

The key separation: **the GUI always runs without admin privileges**; privileges are only needed by
the sing-box process (the TUN interface). All runtime control goes over the localhost API — it
needs no privileges.

## Requirements decisions

### Admin privileges without constant UAC prompts

Install sing-box as a **system service**; elevation is needed once — when installing/registering
the service.

- **Windows**: Windows Service (`sc create` or the `windows-service` crate). Service installation is
  the single UAC prompt. After that, the GUI starts/stops the service via the Service Control
  Manager: we grant the user the right to control that specific service (`sc sdset` with SDDL at
  install time) — then start/stop needs no UAC at all.
- **Linux**: a systemd unit (system-level). Control via `systemctl` + a polkit rule allowing the
  user's group to start/stop the unit without a password. A simpler alternative: `setcap
  cap_net_admin+ep` on the sing-box binary and run it as a user process.
- **macOS**: a launchd daemon (`/Library/LaunchDaemons`), registered via `SMAppService` or with a
  single password prompt when installing the plist. Control — `launchctl kickstart/kill`.

A fallback mode without TUN (a local proxy port only) needs no privileges at all — useful for a
first launch.

### Simple installation

- **Windows** (priority): an NSIS installer from the Tauri toolchain. The installer: installs the
  app, downloads/places the sing-box binary, registers the service (the one UAC prompt). Plus a
  portable zip. Later — winget.
- **Linux**: AppImage + .deb; AUR later.
- **macOS**: .dmg; brew cask later.
- GUI auto-updates via `tauri-plugin-updater`.

### Managing the sing-box binary

- The binary is downloaded from GitHub releases (sha256 verification) and updated independently of
  the GUI: a manual "update" button + optional auto-update (in `settings.json`: `off` / `notify` /
  `auto`). An update = download → `sing-box check` on the current config → stop the service →
  replace → start.
- If `settings.json` specifies a custom binary path, we use it and leave its auto-update alone
  (notifications only).
- **Compatibility matrix**: each Vantage Box release declares a supported sing-box version range
  (semver, e.g. vantage-box 0.0.1 → `~1.1.1`). The version is detected via `sing-box version`.
  Outside the range (e.g. `>1.2.0`) — we keep working but show a warning in the UI; auto-update
  never installs a version outside the range.

### Settings like VS Code (dot-files-friendly)

One readable `settings.json` in the standard config directory:

- Windows: `%APPDATA%/vantage-box/settings.json`
- Linux: `~/.config/vantage-box/settings.json`
- macOS: `~/Library/Application Support/vantage-box/settings.json`

Contents: path to the sing-box `config.json`, path to the sing-box binary (manual; if not set — the
Vantage Box-managed binary), the API address, hotkeys, autostart, theme, tray behavior, binary
auto-update policy. The file is the single source of truth: the settings UI edits it, manual edits
are picked up on the fly via `notify` (file watcher). Comments — support JSONC. The schema —
publish a JSON Schema for editor autocomplete.

### Editing the config

Right in the MVP: a built-in editor — Monaco/CodeMirror with the sing-box JSON Schema
(autocomplete, validation), a `sing-box check` run before applying. Plus an "open config.json in
the system editor" button and file watching → offer a soft restart.

### Control and selectors

- Stop/start/restart of the service (ServiceController, see above).
- Soft restart: before the restart, drop the current selector selections (`GET /proxies`), and
  after start, restore them (`POST /proxies/{tag}`). Account for sing-box caching selections via
  `cache_file` — use it as the first level, with restore on top as a safety net.
- Selectors: group cards, one-click switching, instant, no restart. Group latency test
  (`GET /group/{name}/delay`).

### Logs and stats

- A separate logs screen in the UI: a realtime tape (`/logs`, WS), level filter (errors only),
  pause, search, copy/export, an in-memory ring-buffer (doesn't eat RAM).
- `/traffic` (WS) — a speed chart + counters.
- `/connections` (WS) — an active connections table: domain/IP, outbound, speed; later
  `DELETE /connections/{id}`.

### Global hotkeys and tray

- `tauri-plugin-global-shortcut`: works on all three OSes. Default `Ctrl+Alt+P` — a proxy-selection
  popup menu at the tray; another hotkey for toggle on/off. All bindings live in `settings.json`.
- Tray: the icon changes color/badge by state (off / running / which outbound is active). Menu:
  selectors, toggle, restart, open logs. Closing the window — minimize to the tray.

## Stages

**M0 — skeleton (1 week)**
Tauri 2 + Svelte, `settings.json` (read/watch/schema), ClashApiClient (HTTP+WS), connect to an
already-running sing-box.

**M1 — MVP core (3–4 weeks)**
Dashboard: status, selectors, traffic. Realtime logs screen with a filter. Built-in config editor
(Monaco + JSON Schema + `sing-box check`). ServiceController for Windows (service + SDDL, one UAC
at install). Soft restart preserving selections. On-the-fly secret generation (runtime config
copy). sing-box binary manager: custom path / download, update, compatibility matrix. NSIS
installer.

**M2 — tray and hotkeys (1–2 weeks)**
Tray with a dynamic icon and menu, global hotkeys, proxy-selection popup, autostart, single
instance.

**M3 — cross-platform (2 weeks)**
Linux (systemd/setcap, AppImage/deb), macOS (launchd, dmg). CI: a GitHub Actions build matrix,
auto-updates.

**M4 — later**
Connections table with kill (`DELETE /connections/{id}`), a custom fallback on top of selectors
(ping the active outbound → auto-switch to a backup), subscriptions.

## Risks and notes

- Clash API: bind strictly to `127.0.0.1`. The secret is not stored in user settings — it is
  generated on the fly at each service start: the GUI creates a runtime copy of the config with the
  `experimental.clash_api.secret` injected (the user's `config.json` is untouched), and sing-box is
  started with it. If the user set a secret in the config themselves — we respect it.
- Windows service control rights (SDDL) — the trickiest part; fallback: an elevation prompt only
  for start/stop via a small separate helper.
- The `/logs`, `/traffic`, `/connections` endpoints — WebSocket, not polling.
- WebView2 on Windows is preinstalled on Win10+, but the installer must be able to download it
  (Tauri does this itself).
- Keep no state in the GUI: the source of truth is the sing-box API + `settings.json`. The GUI can
  be killed/restarted at any moment.