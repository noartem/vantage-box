# Integration API — `vantage://` URI & control bus

Vantage Box exposes its actions to external apps — system utilities, Raycast,
browser bookmarks, Windows shortcuts, user scripts — through **two surfaces**
that both reach the exact same handlers as the tray and the GUI:

| Surface | Transport | Best for |
|---|---|---|
| **`vantage://` URI scheme** | The OS launches the app with the URI | Fire-and-forget triggers from web pages, launchers, shortcuts. No response channel. |
| **JSON-RPC control bus** | Windows named pipe, line-delimited JSON-RPC 2.0 | Native clients that want a reply: the CLI, a future PowerShell module, an MCP server. |

The CLI (`vantage-box.exe cli …`) is the reference client for the bus — see
[CLI.md](CLI.md). This document is the complete reference for both surfaces.

## Architecture

The control bus is hosted **inside the running Tauri app** — the tray app is the
broker. The app owns a Windows named pipe and serves JSON-RPC 2.0 over it; the
CLI and URI handler are clients. Handlers are thin wrappers over the same
`actions::*` / `ClashClient` / `window::*` functions the GUI calls, so an
external integration and a tray click behave identically.

```
[CLI]   [vantage://URI]   (future: PowerShell module, MCP server)
   |            |
   | named pipe | single-instance callback (URI) / cold-start argv
   v
[ Vantage Box app = broker ]
   ipc server (tokio named_pipe + JSON-RPC 2.0)
   handlers → actions::* / ClashClient / window::*
   forwards: service://changed → state_changed, proxies://changed → proxies_changed
   |
[ sing-box: Clash API = source of truth ]
```

**Bus lifetime = app lifetime.** The pipe exists only while the app runs. When
the app quits, clients fail to connect (CLI exit `3`). For an always-on tunnel,
use service mode + autostart — see [CLI.md → Headless](CLI.md#headless--always-on).

---

## `vantage://` URI scheme

A fire-and-forget way to ask the running app to do something. The OS launches
`vantage-box.exe "uri" "<url>"`:

- **App already running** — `tauri-plugin-single-instance` routes the second
  launch's args to the first instance, which parses and dispatches the URI.
- **App not running** — the same binary starts normally, finds the URI in
  `argv`, and dispatches after setup (cold-start).

There is **no response channel** — a web page cannot tell whether the action
succeeded. Failures are logged and swallowed. Use the CLI (or a future bus
client) when you need a result.

### Registration

The **NSIS installer** registers the scheme per-user under
`HKCU\Software\Classes\vantage` and removes it on uninstall — no admin needed.
After installing, `vantage://` links work in any browser.

**Portable build** users (no installer) must register it themselves — save this
as a `.reg` file, adjusting the path, and double-click:

```reg
Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\Classes\vantage]
@="URL:Vantage Box"
"URL Protocol"=""

[HKEY_CURRENT_USER\Software\Classes\vantage\shell\open\command]
@="\"C:\\Path\\To\\vantage-box.exe\" \"uri\" \"%1\""
```

### Action reference

Only this fixed whitelist is accepted. Anything that takes a path, a config
blob, or admin rights (`install`, `uninstall`, `set-config`) is **rejected
outright** — a URI that could install a service or overwrite config would be a
phishing vector.

| URI | Action | Confirm? | Description |
|---|---|---|---|
| `vantage://start` | Start | yes | Start sing-box. |
| `vantage://stop` | Stop | yes | Stop sing-box. |
| `vantage://toggle` | Toggle | yes | Start if stopped, stop if running. |
| `vantage://show` | Show | **no** | Bring the main window to the front. The only no-confirm action. |
| `vantage://status` | Show | **no** | Alias of `show` for the URI surface (status reads have no useful side effect here; use the CLI for status). |
| `vantage://select?group=<g>&node=<n>` | Select | yes | Select node `<n>` in group `<g>`. |

The action is the leading path segment, **case-insensitive**:
`vantage://Toggle`, `vantage://toggle/`, `vantage://toggle?x=1` are all `toggle`.

### `select` parameters

`select` takes query-string parameters:

- `group` — the group name (required)
- `node` — the node name (preferred key)
- `name` — accepted as a lenient alias for `node`

```
vantage://select?group=proxy&node=hk-01
vantage://select?group=proxy&name=hk-01
```

**Percent-encoding** is decoded (`name=Caf%C3%A9` → `Café`). Both `group` and
`node` must be:

- non-empty,
- ≤ 256 characters,
- free of `/` and `\` (rejected **even when percent-encoded** — a node name
  containing a path separator is not a real node),
- free of control characters (including NUL — `%00` is rejected).

Unknown actions, non-`vantage://` schemes, and malformed `select` URIs are
logged and ignored — no dialog, no state change.

### Confirmation

State-changing actions (`start`, `stop`, `toggle`, `select`) trigger an in-app
dialog before running:

> An external link wants to: **start**.
> Allow?

`show` / `status` run with no prompt. Dismissing the dialog is a silent no-op.
This is the gate against the web being an untrusted source — a page can ask, but
the user always decides.

### Examples

```html
<!-- A bookmarkable link on a personal page / dashboard -->
<a href="vantage://toggle">Toggle VPN</a>
<a href="vantage://select?group=proxy&node=hk-01">HK-01</a>
```

```bash
# From the Run dialog (Win+R), a shortcut target, or a script:
vantage-box.exe uri "vantage://show"
vantage-box.exe uri "vantage://select?group=proxy&node=hk-01"

# Or, once registered, just open the URL:
start vantage://toggle
```

**Raycast / launcher:** add a "Deeplink" command with the URL
`vantage://select?group=proxy&node=hk-01`, or a script that calls
`vantage-box.exe cli …` when you need the result — see [Integration recipes](#integration-recipes).

---

## JSON-RPC control bus

For clients that need a reply. Line-delimited JSON-RPC 2.0 over a Windows named
pipe. The CLI is the reference implementation — read `src-tauri/src/cli.rs` for
a ~100-line client.

### Transport

| | |
|---|---|
| Pipe path | `\\.\pipe\vantage-box\control` |
| Protocol | JSON-RPC 2.0, one object per line, `\n`-separated |
| ACL | Current user + `LocalSystem` only (per-user install — no `Everyone`, no other accounts) |
| Remote clients | Refused at the pipe layer (`reject_remote_clients`) |

A client opens the pipe, writes one `Request` line, and reads lines until a
`Response` with the matching `id` arrives (skipping any `Notification`s).

### Messages

**Request** (client → server):
```json
{"jsonrpc":"2.0","id":1,"method":"select","params":{"group":"proxy","name":"hk-01"}}
```
`id` may be a number or a string. `params` is a JSON value (object for methods
that take parameters, `{}` or omitted otherwise). The server is lenient about a
missing `jsonrpc` field on input.

**Response** (server → client) — exactly one of `result` / `error`:
```json
{"jsonrpc":"2.0","id":1,"result":null}
{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"invalid params: …"}}
```

**Notification** (server → client, no `id`) — pushed to all connected clients:
```json
{"jsonrpc":"2.0","method":"state_changed","params":{"running":true,…}}
```

### Method catalogue

| Method | Params | Result | Notes |
|---|---|---|---|
| `start` | — | `RunStatus` | Start sing-box. |
| `stop` | — | `RunStatus` | Stop sing-box. |
| `toggle` | — | `RunStatus` | Start if stopped, stop if running. |
| `restart` | — | `RestartOutcome` | Soft restart: snapshot selections, restart, reapply. |
| `installService` | — | `RunStatus` | Install the SCM service (**UAC** — uses the existing PowerShell path). |
| `uninstallService` | — | `RunStatus` | Uninstall the SCM service (**UAC**). |
| `status` | — | `StatusReport` | Combined run + connection status. |
| `runtimeConfig` | — | `RuntimeConfigView` | Read-only runtime config. |
| `proxies` | — | `ProxyOverview` | Groups with nodes and latest latency. |
| `select` | `{group, name}` | `null` | Select a node in a group. |
| `testDelay` | `{name}` | number (ms) | Latency of one node. |
| `testGroupDelay` | `{group}` | map node→ms | Latency of every node in a group. |
| `connections` | — | `ConnectionsSnapshot` | Active connections + totals. |
| `closeConnection` | `{id}` | `null` | Close one connection. |
| `closeAllConnections` | — | `null` | Close all connections. |
| `subscriptions.refresh` | `{force}` | `ApplyOutcome` | Re-pull subscriptions, re-inject nodes. |
| `subscriptions.state` | — | `SubscriptionsState` | Subscription state. |
| `ui.showMain` | — | `null` | Bring the main window to the front. |
| `ui.togglePopup` | — | `null` | Toggle the cursor popup. |
| `ui.closePopup` | — | `null` | Close the cursor popup. |

Result shapes are documented with examples in [CLI.md → Result shapes](CLI.md#result-shapes)
(run `vantage-box.exe cli <cmd> --json` to see any of them verbatim).

### Notifications

A long-lived client (e.g. a future MCP server) can subscribe to events instead
of polling. Each connected pipe client automatically receives them:

| Method | Params | When |
|---|---|---|
| `state_changed` | `RunStatus` | sing-box started/stopped/changed mode. |
| `proxies_changed` | `null` | A selection changed — re-fetch `proxies`. |

`proxies_changed` carries no payload on purpose: re-fetch `/proxies` to get the
new state. A slow client that falls behind simply misses events (no backpressure
on the bus).

### Error codes

| Code | Meaning |
|---|---|
| `-32700` | Parse error — bad JSON on the wire. |
| `-32600` | Invalid request (reserved). |
| `-32601` | Method not found. |
| `-32602` | Invalid params. |
| `-32603` | Internal error. |
| `-32000` | Bus unavailable — sing-box is not running or the Clash API is unreachable. |
| `-32001` | Unauthorized — the Clash API rejected the bearer secret (HTTP 401). |
| `-32002` | Not applicable — the action does not apply in the current mode (e.g. install while running). |
| `-32003` | Timeout (reserved — the `--wait` path maps to CLI exit 5 instead). |
| `-32004` | URI cancelled (reserved — the URI surface has no response channel). |

The `-32xxx` band follows the JSON-RPC 2.0 convention (server-defined errors).

### A minimal client

The protocol is dependency-free (`serde_json` is enough). Sketch:

```rust
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

let pipe = ClientOptions::new().open(r"\\.\pipe\vantage-box\control")?;
let (read, mut write) = tokio::io::split(pipe);
let mut reader = BufReader::new(read);

// Send one request.
let req = r#"{"jsonrpc":"2.0","id":1,"method":"status","params":{}}"#;
write.write_all(format!("{req}\n").as_bytes()).await?;

// Read until our id comes back (skip notifications).
let mut line = String::new();
loop {
    line.clear();
    reader.read_line(&mut line).await?;
    let v: serde_json::Value = serde_json::from_str(line.trim())?;
    if v.get("id") == Some(&serde_json::json!(1)) { break; } // got our response
}
```

> `ClientOptions::open` is **synchronous** and fails fast with
> `ERROR_FILE_NOT_FOUND` when the pipe (the app) is not running — map that to a
> "bus unavailable" state, don't hang.

---

## Integration recipes

### Browser bookmark / dashboard

The simplest integration — just a link:
```html
<a href="vantage://toggle">Toggle VPN</a>
```
Clicking it opens the confirmation dialog (for state-changing actions) or just
the window (`vantage://show`). No scripting, no result.

### Windows shortcut / Run dialog

Create a shortcut with target:
```
"C:\Program Files\Vantage Box\vantage-box.exe" uri "vantage://select?group=proxy&node=hk-01"
```
Pin it to the taskbar, bind a global hotkey to it, or run it from Win+R. This
works even before the scheme is registered (it calls the binary directly).

### Shell / PowerShell script (needs a result)

Use the CLI with `--json` when you want to branch on the outcome:
```powershell
$status = vantage-box.exe cli status --json | ConvertFrom-Json
if ($status.run.running) {
    vantage-box.exe cli stop --wait
} else {
    vantage-box.exe cli start --wait
}

# Switch to the lowest-latency node in a group.
$group = "proxy"
$delays = vantage-box.exe cli test-group-delay $group --json | ConvertFrom-Json
$best  = $delays.PSObject.Properties | Where-Object Value -gt 0 | Sort-Object Value | Select-Object -First 1
vantage-box.exe cli select $group $best.Name
```

### Raycast / launcher

- **Deeplink action:** point a command at `vantage://toggle` (one click, with
  confirmation) or `vantage://show` (no confirmation).
- **Script action** (needs a result): call `vantage-box.exe cli …` and parse.

### Future native clients

The bus is designed for more clients than just the CLI. A PowerShell module and
an MCP server are the natural next surfaces — both are thin clients of the same
pipe and method catalogue above, no new server-side work required.

---

## Security & limitations

- **ACL:** only the current user and `LocalSystem` can reach the pipe. Not
  `Everyone`, not another admin on the machine — the install is per-user, so the
  bus is per-user too. Remote (SMB) pipe clients are refused at the pipe layer.
- **URI whitelist:** only the low-risk actions in the [table](#action-reference)
  are accepted; no paths, no config, no `install`/`uninstall` over URI.
- **URI confirmation:** every state-changing URI action prompts the user; the
  web is untrusted by default.
- **Bus lifetime:** the pipe lives only while the app runs. Headless / always-on
  → service mode + autostart (see [CLI.md](CLI.md#headless--always-on)).
- **No remote access:** the bus is local-only by design. If you need control
  from another machine, run the CLI there over SSH — do not expose the pipe.

## See also

- [CLI.md](CLI.md) — the reference bus client (`vantage-box.exe cli …`), with
  every command, flag, exit code, and `--json` result shape.
- [CONTRIBUTING.md](CONTRIBUTING.md) — development setup.