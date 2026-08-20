# Contributing to Vantage Box

Development, testing and release internals for Vantage Box — a Tauri 2 (Rust) + SvelteKit
(TypeScript) desktop GUI for sing-box. For the user-facing overview, see [README.md](README.md);
for the original plan and architecture decisions, see [PLAN.md](PLAN.md).

## Stack

- **Backend:** Rust, Tauri 2. Crates — `tokio`, `reqwest`, `tokio-tungstenite`, `serde_json`,
  `notify`. Official Tauri plugins: `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`,
  `tauri-plugin-single-instance`, `tauri-plugin-updater`.
- **Frontend:** Svelte + TypeScript + Vite (SPA, `adapter-static`). CodeMirror 6 for the config
  editor. i18n via [`@inlang/paraglide-js`](https://inlang.com/m/2tqxd5cb/paraglide-js-i18n).
- **The GUI always runs without admin privileges.** Elevation is only ever needed to install the
  sing-box service (for TUN configs) — a single UAC prompt, after which start/stop needs no
  privileges (granted to the account via SDDL).

```
┌─────────────────────────────────────────┐
│ Tauri app (user-level, no admin)        │
│  ├─ UI (webview): dashboard, logs,      │
│  │   selectors, config editor           │
│  ├─ Rust core:                          │
│  │   ├─ ClashApiClient (HTTP+WS)        │
│  │   ├─ ServiceController (start/stop)  │
│  │   ├─ Settings (settings.json+watch)  │
│  │   └─ Hotkeys, Tray                   │
└──────────────┬──────────────────────────┘
               │ process control + localhost Clash API
┌──────────────▼──────────────────────────┐
│ sing-box (system service when TUN)      │
│  └─ Clash API on 127.0.0.1:9797         │
└─────────────────────────────────────────┘
```

## Requirements

- **Rust** (stable) and the Tauri 2 system dependencies.
- **Node.js 20+**.
- A running **sing-box** with `experimental.clash_api` enabled for live testing.

The app talks to the Clash API on **127.0.0.1:9797**, not the Clash-common 9090: on 9090 it is very
likely to connect to a sing-box it did not start itself. An `…:9090` address in `settings.json` is
rewritten to the app's own on read.

## Development

```bash
npm install
npm run tauri dev
```

Checks:

```bash
npm run check          # paraglide compile + svelte-check + i18n lint
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
streams. Without the environment variable, a plain `cargo test` skips this test.

### App smoke test

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/smoke-test.ps1
```

This checks what is invisible from the outside: whether the tray icon came up, whether global
hotkeys could be claimed, whether settings were read, and whether the popup opens. The app prints a
line like

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
into `SINGBOX_MIN` / `SINGBOX_MAX_EXCLUSIVE` in
[client.rs](src-tauri/src/clash/client.rs). Each version is checked in its own sandbox; binaries are
cached. A specific list of versions: `-Versions 1.11.15,1.12.9,1.13.16`.

> The probes for the matrix and for the integration test are the same and live in
> [compat.rs](src-tauri/src/compat.rs), so they cannot drift apart.

## Settings

`settings.json` is the single source of truth — the UI and manual edits in the editor are equal,
changes are picked up on the fly.

| OS | Path |
|---|---|
| Windows | `%APPDATA%\vantage-box\settings.json` |
| Linux | `~/.config/vantage-box/settings.json` |
| macOS | `~/Library/Application Support/vantage-box/settings.json` |

The schema lives next to it (`settings.schema.json`) and is wired up via `$schema`, so autocomplete
works offline. Comments and trailing commas are allowed (JSONC).

## Release

Releases are built by GitHub Actions (Windows-only) on the `v*` tag — see
[build.yml](.github/workflows/build.yml). `tauri-action` builds the NSIS installer, signs it, and
creates a release with the installer + `.sig` + `latest.json` (for auto-update); a separate step
builds the portable zip. After publishing, the workflow pokes the [Scoop bucket](https://github.com/noartem/bucket)
so Excavator updates the `vantage-box` manifest via `checkver`/`autoupdate`. Fast checks on every
push/PR run in [ci.yml](.github/workflows/ci.yml).

### Update signing key

Auto-update verifies the package signature against the public key embedded in
[tauri.conf.json](src-tauri/tauri.conf.json) → `plugins.updater.pubkey`. The paired private key is a
secret and must not be in the repo.

To sign releases, add two secrets to the repository settings (`noartem/vantage-box` → Secrets and
variables → Actions):

- `TAURI_SIGNING_PRIVATE_KEY` — the private key contents (the file `~/.tauri/vantage-box.key`,
  generated by `tauri signer generate`).
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the key password. The current key was generated without a
  password, so the secret is left empty.

Generate a new pair locally (e.g. when changing the password):

```bash
npx tauri signer generate --ci -w ~/.tauri/vantage-box.key --password "your-password"
```

Put the public key from `.key.pub` into `tauri.conf.json`. Rotating the key means copies installed
with the old key will no longer update.

## Conventions

A few non-obvious rules the codebase follows — read before changing things.

- **Never modify the user's `config.json`.** The app works through a runtime copy with a one-time
  secret appended; key order is preserved and our block is appended at the end or replaces an
  existing value. Anything that mutates the user's file is a bug.
- **Never touch a running sing-box that the app didn't start.** Tests run against a separate,
  isolated process (different ports, temp config dir via `VANTAGE_BOX_CONFIG_DIR`, no TUN). Live
  integration tests are env-gated and pass vacuously without `VANTAGE_BOX_TEST_SINGBOX`.
- **i18n keys are flat, not dotted.** In `messages/*.json` use `tabs_dashboard`, not
  `tabs.dashboard` — dotted keys don't resolve through Paraglide's `m.x()`. `npm run check` lints
  this.
- **Frontend messages are compiled, not hand-written.** Run `npm run i18n:compile`
  (part of `npm run check`) after editing `messages/*.json` so `src/lib/paraglide` regenerates.
- **Keep the compat probes in sync.** The integration test and the compatibility matrix share
  [compat.rs](src-tauri/src/compat.rs) — add new probes there once, not in two places.
- **Commits follow Conventional Commits** (`feat:`, `fix:`, …) — match the existing log.
- **The range bounds are measured.** `SINGBOX_MIN` / `SINGBOX_MAX_EXCLUSIVE` come from
  `scripts/compat-matrix.ps1` output, not from a guess. Update them only with fresh matrix results.

## License

MIT. Note: a `LICENSE` file is referenced from `package.json` but not yet committed — add one when
convenient.