<p align="center">
  <img src="static/logo-on.svg" alt="Vantage Box" width="120" height="120" />
</p>

<h1 align="center">Vantage Box</h1>

<p align="center">
  A minimal desktop GUI for <a href="https://github.com/SagerNet/sing-box">sing-box</a>.<br/>
  Bring your own <code>config.json</code>.
</p>

<p align="center">
  <a href="https://github.com/noartem/vantage-box/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/noartem/vantage-box/actions/workflows/ci.yml/badge.svg"/></a>
  <a href="https://github.com/noartem/vantage-box/actions/workflows/build.yml"><img alt="Build" src="https://github.com/noartem/vantage-box/actions/workflows/build.yml/badge.svg"/></a>
  <a href="https://github.com/noartem/vantage-box/releases"><img alt="Release" src="https://img.shields.io/github/v/release/noartem/vantage-box?include_prereleases"/></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-blue"/>
  <img alt="License" src="https://img.shields.io/badge/license-MIT-green"/>
  <img alt="sing-box" src="https://img.shields.io/badge/sing--box-1.10.7%E2%80%931.13.x-orange"/>
</p>

---

## Features

**Bring your own config.** Point Vantage Box at any existing sing-box `config.json`. It never
rewrites your file — it works through the Clash API and keeps a separate runtime copy.

**Control it from the tray.** Start, stop, soft-restart and switch proxy groups straight from the
tray icon — no need to open the window. The icon's color reflects the running state.

**Hotkeys & a cursor popup.** Bind global shortcuts for the actions you use most. A frameless
proxy-selection popup appears right at the cursor for a one-key node switch; it closes on Esc or
focus loss.

**See what's happening, live.** Real-time traffic, logs and memory streams, plus an active
connections table you can filter and close — one connection or all at once.

**Edit the config in the app.** A built-in editor with JSON Schema autocomplete, `sing-box check`
validation before saving, automatic `.bak` backups, and live reload when the file changes externally.

**Manage sing-box versions.** Browse the GitHub release catalog, install any version with one click,
and switch — the config is re-validated against the new version and sing-box is restarted for you.

**Subscriptions.** Add a subscription URL (sing-box JSON or a base64 list of `ss`/`vmess`/`vless`/
`trojan`/`hysteria2`/`tuic` URIs). Nodes are injected into your selector/urltest groups and kept
updated on a schedule or on demand — without redundant restarts.

**Automatic fallback.** The active node of a selector group is pinged periodically; on failure or a
latency breach Vantage Box switches to a backup automatically. `urltest` groups are left alone.

**Lives in the background.** Autostart, start minimized, single instance (a second launch just
focuses the open window), and minimize-to-tray on close.

**Updates itself.** Signed, signature-verified auto-updates — choose "don't check / notify / install
automatically" in settings.

## Installation

> The sing-box binary is **not** bundled. On first run Vantage Box downloads its own, or you point it
> at an existing one.

### Scoop (recommended)

```bash
scoop bucket add noartem https://github.com/noartem/bucket
scoop install noartem/vantage-box
```

Scoop keeps it up to date.

### From a release

Grab the **NSIS installer** (`*-setup.exe`) or the **portable zip** (a single `vantage-box.exe` with
no installation) from the [Releases](https://github.com/noartem/vantage-box/releases) page. Installer
builds are signed and self-update; the portable build checks for updates and notifies you.

## sing-box compatibility

Vantage Box works with sing-box **1.10.7 – 1.13.x**. Outside the range it keeps running but shows a
warning, and built-in version auto-update never crosses the range boundaries.

The range is measured, not guessed: a compatibility matrix script runs the same probe set against
every minor release. Last run (August 7, 2026) — all 10 probes passed on every version:

| version | result |
|---|---|
| 1.10.7 | ✅ OK |
| 1.11.15 | ✅ OK |
| 1.12.25 | ✅ OK |
| 1.13.16 | ✅ OK |

> The lower bound is the oldest **verified** version, not the oldest working one: we don't promise
> what we haven't measured.

---

<p align="center">
  <strong>MIT License</strong> · Development docs in <a href="CONTRIBUTING.md">CONTRIBUTING.md</a><br/>
  Not affiliated with <a href="https://github.com/SagerNet/sing-box">sing-box</a>.
</p>
