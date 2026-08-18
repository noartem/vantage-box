# Vantage Box

A minimal desktop GUI for [sing-box](https://github.com/SagerNet/sing-box). It takes
your existing `config.json` and drives the runtime through the Clash API — no config format
of its own.

The development plan and requirements decisions are in [PLAN.md](PLAN.md).

## Status

**M0 — skeleton** and most of **M1 — MVP core**:

- Tauri 2 + SvelteKit (SPA, adapter-static) + TypeScript.
- `settings.json` in the standard OS config directory: JSONC reading, atomic writes,
  live-reload via a file watcher, JSON Schema for editor autocomplete.
- `ClashApiClient`: HTTP (`/version`, `/proxies`, `/group/{name}/delay`, `/configs`) and three
  WebSocket streams (`/traffic`, `/logs`, `/memory`) with reconnection.
- Two launch modes. A config with TUN requires administrator privileges — for it, sing-box is
  registered as a service (a single UAC prompt, then start/stop needs no privileges: they are
  granted to the account via SDDL). A config without TUN runs as an ordinary child process; no
  service installation is needed.
- A runtime copy of the config with a one-time secret. The user's `config.json` is never modified;
  the copy differs from it minimally: key order is preserved, our block is appended at the end or
  replaces an existing value.
- Built-in config editor: CodeMirror 6, JSON Schema, `sing-box check` before saving, a `.bak`
  backup, and watching for external edits.
- sing-box version manager: the GitHub release catalog is cached to disk and shown from cache,
  each version is stored as a separate file, switching validates the config against the new
  version and restarts sing-box.
- Soft restart: selector-group selections are dropped before stopping and restored afterwards.
- UI: dashboard, config, logs, service, settings.

**M2 — tray and hotkeys**:

- Tray icon: color reflects state (running — normal, otherwise grey and semi-transparent), a menu
  with selector groups, start/stop, soft restart, and quit.
- Global hotkeys from `settings.json`, re-registered on the fly. Conflicting combinations are
  shown in settings instead of failing silently.
- A proxy-selection popup at the cursor: a separate frameless window, closes on Esc and on focus
  loss.
- Autostart, single instance (a second launch brings the already-open window to the front),
  minimizing to the tray on window close, starting minimized.

**M3 — CI and auto-update** and **M4 — connections, subscriptions, fallback**:

- NSIS installer (Tauri, `installMode: currentUser`); the sing-box binary is not bundled into the
  installer — the app downloads its own or the user provides a path.
- App auto-update via `tauri-plugin-updater`: signature verification, "don't check / notify /
  install automatically" modes in `settings.json` (`guiUpdate.policy`).
- Builds via GitHub Actions: `ci.yml` (check + test + build on every push), `release.yml`
  (NSIS + `.sig` + `latest.json` + portable zip on the `v*` tag). Windows-only.
- Active connections table with filtering and closing one-by-one or all at once (WS `/connections`).
- Subscriptions: a URL returns sing-box JSON or a base64 list of URIs
  (`ss`/`vmess`/`vless`/`trojan`/`hysteria2`/`tuic`). Nodes are injected into `config.json` under
  `sub:<id>:` tags, appended to selector/urltest groups, and applied via a soft restart. Updates
  run on an interval or manually; redundant restarts are avoided by signing the node set.
- Fallback: periodic pinging of the active node of a selector group and automatic switching to a
  backup on failure or latency threshold breach. `urltest` groups are not affected.

Not done yet: Linux/macOS builds in CI (the runtime service control there is stubbed out).

## Requirements

- Rust (stable) and the Tauri 2 system dependencies.
- Node.js 20+.
- A running sing-box with `experimental.clash_api` enabled.

The minimal sing-box config snippet the app needs:

```json
{
  "experimental": {
    "clash_api": {
      "external_controller": "127.0.0.1:9797",
      "secret": ""
    }
  }
}
```

If `secret` is set — enter it in Vantage Box settings. An empty secret means Vantage Box will
generate its own on each launch.

The default port is 9797, not the Clash-common 9090: on 9090 the app is very likely to connect to
a sing-box it did not start itself. An `…:9090` address in `settings.json` is rewritten to the
app's own on read.

## Development

```bash
npm install
```

```bash
npm run tauri dev
```

Checks:

```bash
npm run check
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

### Integration test against a real sing-box

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/integration-test.ps1
```

The test starts a **separate** sing-box process and never touches an already-running one: different
ports (19090/19080), a config without TUN (so no admin privileges and no network-stack
interference), all state kept in a temp folder via `VANTAGE_BOX_CONFIG_DIR`. Only its own child
process is killed. The script finds the binary on its own (including resolving scoop shims) and
writes output to `test-results/`.

What is checked: version detection, runtime config build, immutability of the user's file,
`sing-box check`, `/version`, `/proxies`, selector switching, and the `/traffic` and `/logs`
streams.

Without the environment variable, a plain `cargo test` skips this test.

### App smoke test

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/smoke-test.ps1
```

This checks what is invisible from the outside: whether the tray icon came up, whether global
hotkeys could be claimed, whether settings were read, and whether the popup opens. The app prints
a line like

```
vantage-box startup tray=ok hotkeys=ok window=shown settings=ok
```

and the `--self-test` flag makes it open the popup, wait for a signal from the loaded webview, and
exit. A debug build pulls the frontend from the Vite dev server — the script starts it on its own.
To check a release build with the bundled frontend: `-Release`.

### sing-box compatibility matrix

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/compat-matrix.ps1
```

Downloads the latest sing-box releases (one per minor branch), runs the same probe set as the
integration test against each, and writes `test-results/compat-matrix.md` and `compat-matrix.json`.
At the end it prints the version range derived **from the measurements** — that is what should go
into `SINGBOX_MIN` / `SINGBOX_MAX_EXCLUSIVE`.

Each version is checked in its own sandbox; binaries are cached. A specific list of versions:
`-Versions 1.11.15,1.12.9,1.13.16`.

## Release

Releases are built by GitHub Actions (Windows-only) on the `v*` tag — see
[release.yml](.github/workflows/release.yml). `tauri-action` builds the NSIS installer, signs it,
and uploads the installer + `.sig` + `latest.json` (for auto-update) to the release; a separate
step builds the portable zip. CI on every push/PR — [ci.yml](.github/workflows/ci.yml).

### Update signing key

Auto-update verifies the package signature against the public key embedded in
[tauri.conf.json](src-tauri/tauri.conf.json) → `plugins.updater.pubkey`. The paired private key is
a secret and must not be in the repo.

To sign releases, add two secrets to the repository settings (`noartem/vantage-box` → Secrets and
variables → Actions):

- `TAURI_SIGNING_PRIVATE_KEY` — the private key contents (the file
  `~/.tauri/vantage-box.key`, generated by `tauri signer generate`).
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the key password. The current key was generated without a
  password, so the secret is left empty.

You can generate a new pair locally (e.g. when changing the password):

```bash
npx tauri signer generate --ci -w ~/.tauri/vantage-box.key --password "your-password"
```

Put the public key from `.key.pub` into `tauri.conf.json`. Note: rotating the key means copies
installed with the old key will no longer update.

## Settings

The file is the single source of truth. The UI and manual edits in the editor are equal: changes
are picked up on the fly.

- Windows: `%APPDATA%\vantage-box\settings.json`
- Linux: `~/.config/vantage-box/settings.json`
- macOS: `~/Library/Application Support/vantage-box/settings.json`

The schema lives next to it (`settings.schema.json`) and is wired up via `$schema`, so autocomplete
works offline. Comments and trailing commas are allowed in the file.

## sing-box compatibility

A release declares the tested sing-box version range
(`SINGBOX_MIN` / `SINGBOX_MAX_EXCLUSIVE` in
[client.rs](src-tauri/src/clash/client.rs)). Outside the range the app keeps working but shows a
warning, and binary auto-update never crosses the range boundaries.

The range is not picked by eye: it comes from `scripts/compat-matrix.ps1` (see above). The probes
for the matrix and for the integration test are the same and live in
[compat.rs](src-tauri/src/compat.rs), so they cannot drift apart.

Last run (August 7, 2026) — all 10 probes passed on every version:

| version | result |
|---|---|
| 1.10.7 | OK |
| 1.11.15 | OK |
| 1.12.25 | OK |
| 1.13.16 | OK |

The lower bound is the oldest **verified** version, not the oldest working one: we don't promise
what we haven't measured.