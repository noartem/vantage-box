# CLI — `vantage-box cli`

Control Vantage Box from the command line. The CLI is a **subcommand of the same
binary** — `vantage-box.exe cli …` — and is the reference client for the app's
local control bus. It drives the exact same actions as the tray and the GUI, so
anything you can click you can also script.

```bash
vantage-box.exe cli status
vantage-box.exe cli start --wait
vantage-box.exe cli select proxy hk-01
```

## Prerequisites

**The app must be running.** The control bus is hosted *inside* the running Tauri
app (the tray app is the broker). When the app quits, the bus goes away and every
CLI call exits `3` (bus unavailable). This is the documented lifetime limitation —
see [Headless / always-on](#headless--always-on) below for the recipe that keeps
the tunnel up without the GUI.

There is nothing extra to install: the CLI is the same `vantage-box.exe` you
already have (Scoop, NSIS installer, or the portable build). It does **not**
initialize Tauri, the tray, the single-instance plugin, or any window — it
connects to the pipe, does one call, and exits.

## Usage

```
vantage-box.exe cli [OPTIONS] <COMMAND>
```

### Options

| Option | Default | Description |
|---|---|---|
| `--json` | off | Emit machine-readable JSON to stdout instead of human text. This is the **integration contract** — stable shape, parse it in scripts. |
| `--wait` | off | For `start` / `stop` / `toggle` / `restart`: poll `status` until the target state is reached instead of returning the moment the call is accepted. See [`--wait`](#--wait). |
| `--timeout <SECS>` | `30` | Seconds to wait when `--wait` is set before giving up (exit `5`). |
| `-h` / `--help` | — | Print help (clap exits `0`). |

### Commands

| Command | Maps to | Description |
|---|---|---|
| `status` | `status` | Combined run + connection status. |
| `start` | `start` | Start sing-box (service or process, depending on the config). |
| `stop` | `stop` | Stop sing-box. |
| `toggle` | `toggle` | Start if stopped, stop if running. |
| `restart` | `restart` | Soft restart: snapshot selector selections, restart, reapply. |
| `proxies` | `proxies` | List selector/urltest groups and their current selection. |
| `select <group> <name>` | `select` | Select a node in a group. |
| `test-delay <name>` | `testDelay` | Measure latency (ms) of a single node. |
| `test-group-delay <group>` | `testGroupDelay` | Measure latency (ms) of every node in a group. |
| `connections` | `connections` | Show active connections. |
| `close-connection <id>` | `closeConnection` | Close one connection by id. |
| `close-all-connections` | `closeAllConnections` | Close all connections. |
| `refresh [--force]` | `subscriptions.refresh` | Re-pull subscriptions and re-inject nodes. `--force` re-fetches even when the remote content is unchanged. |
| `subs` | `subscriptions.state` | Show subscription state. |
| `show` | `ui.showMain` | Bring the main window to the front. |

Run `vantage-box.exe cli --help` for the canonical list generated from the binary
itself.

## Exit codes

The exit code is part of the contract — branch on it in scripts.

| Code | Meaning |
|---|---|
| `0` | OK. |
| `1` | Action error (the call reached the app but failed, e.g. the node does not exist). |
| `2` | Bad arguments / `--help` (clap exits with this on its own). |
| `3` | Bus unavailable — the app is not running (or the pipe is gone). |
| `4` | Unauthorized — the Clash API rejected the bearer secret (HTTP 401). |
| `5` | `--wait` timed out before the target state was reached. |

## Output

### Human-readable (default)

Each command prints a one-or-two-line summary. `status`:

```
mode=service running=true tun=true conn=connected
  sing-box: 1.12.25
```

`proxies`:

```
proxy (Selector) → hk-01
auto (URLTest) → jp-02
```

Errors go to **stderr** (`vantage-box cli: <message>`); the machine-readable
stdout stream stays clean for piping.

### `--json` (the integration contract)

With `--json`, the result is printed as **compact JSON to stdout** — one object,
no surrounding text. Parse this in scripts; do not scrape the human output.

```bash
vantage-box.exe cli status --json
```
```json
{"run":{"mode":"service","running":true,"service":{"installed":true,"running":true},"processPid":null,"tun":true,"configProblem":null},"connection":{"state":"connected","version":"1.12.25","error":null,"compatibility":"supported"}}
```

On error with `--json`, the full JSON-RPC error object is printed to **stdout**
(so `jq` sees it) and a non-zero exit code is returned:

```json
{"code":-32001,"message":"unauthorized"}
```

A bus-unavailable error is emitted as `{"code":-32000,"message":"…","hint":"is the vantage-box app running?"}`.

### Result shapes

The `--json` output is the serde-serialized return value of the underlying
handler (all camelCase, matching the GUI). Key fields:

<details>
<summary><code>status</code> → StatusReport</summary>

```jsonc
{
  "run": {                         // RunStatus
    "mode": "service",             // "service" | "process"
    "running": true,
    "service": {                   // ServiceInfo — the SCM service wrapper
      "name": "VantageBoxSingBox",
      "supported": true,           // a service impl exists for this OS
      "state": "running",          // "notInstalled"|"stopped"|"startPending"|"running"|"stopPending"|"unknown"
      "canControl": true           // can start/stop without elevation
    },
    "processPid": null,            // PID when run as a GUI-owned child
    "tun": true,                   // config needs TUN → needs service + admin
    "configProblem": null          // why the config couldn't be read, if at all
  },
  "connection": {                  // ConnectionStatus
    "state": "connected",          // "disconnected" | "connecting" | "connected"
    "version": "1.12.25",
    "error": null,
    "compatibility": "supported"
  }
}
```
</details>

<details>
<summary><code>start</code> / <code>stop</code> / <code>toggle</code> → RunStatus</summary>

```jsonc
{
  "mode": "service",
  "running": true,
  "service": { "name": "VantageBoxSingBox", "supported": true, "state": "running", "canControl": true },
  "processPid": null,
  "tun": true,
  "configProblem": null
}
```
</details>

<details>
<summary><code>restart</code> → RestartOutcome</summary>

```jsonc
{
  "status": { /* RunStatus */ },
  "restored": ["proxy → hk-01"],   // selections reapplied
  "skipped": [],                   // selections that could not be restored
  "apiBack": true                  // Clash API came up before the wait elapsed
}
```
</details>

<details>
<summary><code>proxies</code> → ProxyOverview</summary>

```jsonc
{
  "groups": [
    {
      "name": "proxy",
      "kind": "Selector",          // "Selector", "URLTest", …
      "now": "hk-01",              // currently selected node, or null
      "selectable": true,          // can be changed by hand
      "items": [
        { "name": "hk-01", "kind": "Shadowsocks", "delay": 42, "udp": true, "isGroup": false },
        { "name": "jp-02", "kind": "VMess",      "delay": 88, "udp": true, "isGroup": false }
      ]
    }
  ]
}
```
`delay` is the latest measurement in ms, or `null` if not measured / no response.
</details>

<details>
<summary><code>connections</code> → ConnectionsSnapshot</summary>

```jsonc
{
  "connections": [ /* one entry per active connection */ ],
  "downloadTotal": 1234567,
  "uploadTotal": 76543
}
```
</details>

<details>
<summary><code>test-delay</code> / <code>test-group-delay</code></summary>

`test-delay` → a number (ms):
```json
42
```

`test-group-delay` → a map of node → ms:
```json
{ "hk-01": 42, "jp-02": 88, "sg-03": 0 }
```
A `0` means the node did not respond, not an instant reply.
</details>

`select`, `close-connection`, `close-all-connections`, `show` return `null` on
success. `refresh` returns `{ "changed": bool, "restarted": bool }`. `subs`
returns `{ "entries": { … }, "applyPending": bool }`.

For the full method catalogue and the JSON-RPC protocol underneath, see
[API.md](API.md).

## `--wait`

`start` / `stop` / `toggle` / `restart` return as soon as the app accepts the
call — the tunnel may still be coming up or tearing down. Add `--wait` to poll
`status` every 400 ms until the target state is reached:

- `start` / `restart` → wait until `run.running == true`
- `stop` → wait until `run.running == false`
- `toggle` → wait for whatever state the call reports it flipped to

If the deadline (`--timeout`, default 30 s) passes first, exit `5`. A transient
RPC error right after `restart` (the API mid-restart) is expected and keeps
polling; the bus dropping mid-poll exits `3`.

`--wait` is **not** the default — a plain `start` returns immediately, which is
what you usually want when chaining commands yourself.

## Examples

```bash
# Read-only checks — safe any time the app is running, no tunnel state change.
vantage-box.exe cli status
vantage-box.exe cli status --json | jq -r '.run.running'
vantage-box.exe cli proxies

# Switch node, then confirm it took.
vantage-box.exe cli select proxy hk-01
vantage-box.exe cli proxies

# Start and block until the tunnel is actually up.
vantage-box.exe cli start --wait --timeout 60

# Scripting: branch on the exit code.
if vantage-box.exe cli status >/dev/null 2>&1; then
  echo "app is running"
elif [ $? -eq 3 ]; then
  echo "app is not running"
fi
```

## Integration tips

- **Shell aliases / functions:** wrap the long path in a function:
  ```bash
  vb() { "/c/Program Files/Vantage Box/vantage-box.exe" cli "$@"; }
  vb status
  ```
- **PowerShell:** call with `--json` and convert:
  ```powershell
  $s = vantage-box.exe cli status --json | ConvertFrom-Json
  $s.run.running
  ```
- **Exit-code driven logic** is more robust than parsing text — use the
  [exit codes](#exit-codes) table, especially `3` (app not running) vs `0`.
- For fire-and-forget triggers from browsers, launchers, or shortcuts, the
  `vantage://` URI scheme is usually simpler — see [API.md](API.md).

## Headless / always-on

The bus lives only while the app runs, so the CLI cannot bring the tunnel up on
its own when the app is closed. For an always-on tunnel that survives logout /
no-GUI sessions:

1. Install the SCM service (from the GUI, or `cli` is not needed here — the
   service is what holds sing-box up independently of the GUI).
2. Put Vantage Box in **autostart** (Settings) so the app — and with it the bus —
   is always there for the CLI to talk to.

The CLI then talks to the running app, which in turn manages the service-backed
tunnel.

## See also

- [API.md](API.md) — the `vantage://` URI scheme and the full JSON-RPC control-bus
  API (method catalogue, error codes, notifications) for building native
  integrations.
- [CONTRIBUTING.md](CONTRIBUTING.md) — development setup.