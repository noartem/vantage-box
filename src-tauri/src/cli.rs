//! The reference control-bus client: `vantage-box.exe cli …`.
//!
//! A subcommand of the same binary, branched in `lib::run()` before
//! `tauri::Builder` — so the CLI never initializes Tauri, the single-instance
//! plugin, or the tray. It connects to the named pipe hosted by the *running*
//! app and drives the same `actions::*` / `ClashClient` handlers the GUI uses.
//!
//! If the app is not running, the pipe is gone and we exit 3 — the documented
//! bus-lifetime limitation. For a tunnel that survives the GUI, use service
//! mode (the app in autostart + the SCM service keeps sing-box up).

use std::fmt::Display;
use std::io;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::ClientOptions;

use crate::ipc::jsonrpc::{Request, RpcError, UNAUTHORIZED};
use crate::ipc::PIPE_NAME;

/// The argv token that selects CLI mode, matching `--scm` / `--self-test`.
pub const FLAG: &str = "cli";

// -- Exit codes (part of the CLI contract; documented in ACTIONS.md) -----------
const EXIT_OK: i32 = 0;
const EXIT_ERROR: i32 = 1;
// Exit 2 is reserved for bad arguments / `--help`: clap exits with 2 on its own
// when `Cli::try_parse_from` rejects the argv, so we never assign it here.
const EXIT_BUS: i32 = 3;
const EXIT_UNAUTHORIZED: i32 = 4;
const EXIT_WAIT_TIMEOUT: i32 = 5;

/// Is this process invocation a CLI call (`vantage-box.exe cli …`)?
///
/// Scans rather than checking `argv[1]` strictly, so a launcher that prepends
/// flags before `cli` does not break us — same reasoning as `scm::is_invocation`.
pub fn is_invocation() -> bool {
    std::env::args().any(|a| a == FLAG)
}

/// Attach to an ancestor's console and re-point stdout/stderr at it.
///
/// The binary is built with `windows_subsystem = "windows"` (the tray app needs
/// no console), so a console host that doesn't hand a GUI-subsystem child
/// working std handles — notably PowerShell, and the scoop shim path — gets
/// silent returns: `println!` writes to a NULL handle. `AttachConsole` borrows
/// a console and we reopen `CONOUT$` as stdout/stderr so output (including
/// clap's `--help`/error text) reaches the terminal.
///
/// `ATTACH_PARENT_PROCESS` only reaches the *immediate* parent. When the app is
/// launched through a handle-less GUI-subsystem launcher — the scoop shim is
/// itself a `windows_subsystem = "windows"` binary with no console — the parent
/// has nothing to lend, so we walk up the process tree and attach to the
/// nearest ancestor that owns a console (the shell the user actually typed in).
///
/// Returns `true` if we borrowed a console (so the caller can `FreeConsole`
/// before exiting). No-op when we already have one (console-subsystem builds,
/// cmd/ConPTY shells that passed usable handles): `GetConsoleWindow` is set,
/// we touch nothing.
#[cfg(windows)]
fn attach_parent_console() -> bool {
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetConsoleWindow, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        ATTACH_PARENT_PROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    unsafe {
        if !GetConsoleWindow().is_null() {
            return false;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            reopen("CONOUT$", STD_OUTPUT_HANDLE, true);
            reopen("CONOUT$", STD_ERROR_HANDLE, true);
            return true;
        }
        // The immediate parent had no console (e.g. the scoop shim). Walk up
        // to the nearest ancestor that does — typically the user's shell.
        let mut pid = match parent_pid(GetCurrentProcessId()) {
            Some(p) => p,
            None => return false,
        };
        for _ in 0..64 {
            if pid == 0 {
                return false;
            }
            if AttachConsole(pid) != 0 {
                reopen("CONOUT$", STD_OUTPUT_HANDLE, true);
                reopen("CONOUT$", STD_ERROR_HANDLE, true);
                return true;
            }
            pid = match parent_pid(pid) {
                Some(p) => p,
                None => return false,
            };
        }
        false
    }
}

/// First ancestor (parent) PID of `pid`, via a process snapshot. `None` if the
/// snapshot can't be taken.
#[cfg(windows)]
unsafe fn parent_pid(pid: u32) -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if snap == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: PROCESSENTRY32W = std::mem::zeroed();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = None;
    if Process32FirstW(snap, &mut entry) != 0 {
        loop {
            if entry.th32ProcessID == pid {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            if Process32NextW(snap, &mut entry) == 0 {
                break;
            }
        }
    }
    CloseHandle(snap);
    found
}

/// Open the console screen buffer and install it as the given std handle. The
/// `File` is forgotten so the OS handle lives until process exit.
#[cfg(windows)]
unsafe fn reopen(name: &str, std_handle: u32, write: bool) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::SetStdHandle;

    let mut opts = std::fs::OpenOptions::new();
    if write {
        opts.write(true);
    } else {
        opts.read(true);
    }
    if let Ok(f) = opts.open(name) {
        let h = f.as_raw_handle();
        std::mem::forget(f);
        SetStdHandle(std_handle, h as *mut _);
    }
}


/// Detach from a console we borrowed with `attach_parent_console`.
#[cfg(windows)]
unsafe fn free_console() {
    use windows_sys::Win32::System::Console::FreeConsole;
    FreeConsole();
}

/// Inject an Enter key into the borrowed console's input buffer.
///
/// `AttachConsole` from a GUI-subsystem process leaves the parent's console in
/// a state where, after we exit, the host's pending `ReadConsole` (PSReadLine,
/// cmd) stays blocked until the user manually presses Enter — the classic
/// "printed then doesn't release the prompt" hang. Feeding one Enter to the
/// input buffer completes that read for it, so the prompt returns on its own.
/// Only called when we actually borrowed a console.
#[cfg(windows)]
unsafe fn release_parent_console() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{
        WriteConsoleInputW, INPUT_RECORD, KEY_EVENT, KEY_EVENT_RECORD,
    };

    // Open the console input buffer with write access.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    let Ok(f) = opts.open("CONIN$") else {
        return;
    };
    let handle = f.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    std::mem::forget(f);

    const VK_RETURN: u16 = 0x0D;
    const SC_RETURN: u16 = 0x1C;
    // One key-down + key-up Enter event. Zeroed first: every field defaults to
    // a harmless value (no modifiers, no char).
    let mut records: [INPUT_RECORD; 2] = std::mem::zeroed();
    // Build a key-down then key-up Enter event. Zeroed first: every field
    // defaults to a harmless value (no modifiers, no char).
    for (idx, &down) in [true, false].iter().enumerate() {
        records[idx].EventType = KEY_EVENT as u16;
        let ke: &mut KEY_EVENT_RECORD = &mut records[idx].Event.KeyEvent;
        ke.bKeyDown = if down { 1 } else { 0 };
        ke.wRepeatCount = 1;
        ke.wVirtualKeyCode = VK_RETURN;
        ke.wVirtualScanCode = SC_RETURN;
        ke.uChar.UnicodeChar = VK_RETURN;
        ke.dwControlKeyState = 0;
    }

    let mut written: u32 = 0;
    WriteConsoleInputW(handle, records.as_ptr(), records.len() as u32, &mut written);
}

#[cfg(not(windows))]
fn attach_parent_console() -> bool {
    false
}
#[cfg(not(windows))]
unsafe fn free_console() {}
#[cfg(not(windows))]
unsafe fn release_parent_console() {}


/// Parse argv (with `program_name` as argv[0] / the displayed binary name), run
/// the command on a one-shot current-thread tokio runtime, flush stdout/stderr,
/// and return the exit code.
///
/// Shared by the GUI `cli` subcommand path and the console `vantage-box-cli`
/// binary. It does NOT touch the console (AttachConsole / FreeConsole is the
/// caller's concern) and does NOT exit — the caller decides the exit strategy
/// so it can release a borrowed console first. A one-shot CLI has nothing to
/// clean up, so callers `std::process::exit(code)` (a runtime drop on the way
/// out can block after all output is done).
fn execute(program_name: &str, args: Vec<String>) -> i32 {
    let mut argv: Vec<String> = Vec::with_capacity(args.len() + 1);
    argv.push(program_name.to_string());
    argv.extend(args);

    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            // clap renders help/version/errors itself and picks the exit code
            // (0 for --help, 2 for usage errors).
            e.exit();
        }
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{program_name}: could not start runtime: {e}");
            return EXIT_ERROR;
        }
    };

    let code = runtime.block_on(run_cli(cli));
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    code
}

/// GUI binary's `cli` subcommand entry (`vantage-box.exe cli …`).
///
/// The binary is `windows_subsystem = "windows"` (the tray app needs no
/// console), so it has no console of its own; `attach_parent_console` borrows
/// one (walking the process tree to the shell when launched through the GUI
/// scoop shim). On exit we feed the host's blocked read an Enter and detach
/// before exiting hard. Prefer the dedicated `vantage-box-cli` console binary
/// when a terminal is available — it has a real console and none of these
/// workarounds. This path remains as a fallback for invoking the CLI through
/// the full path to the GUI binary.
pub fn run() -> i32 {
    let attached = attach_parent_console();

    // Strip everything up to and including the `cli` token, then let clap own
    // the rest. `try_parse_from` treats the first element as argv[0] (the
    // binary name shown in usage); without a placeholder it swallows the real
    // subcommand and exits 2 with a misleading usage line — so prepend one.
    let rest: Vec<String> = std::env::args().skip_while(|a| a != FLAG).skip(1).collect();
    let code = execute("vantage-box cli", rest);

    if attached {
        // Release the parent's blocked console read (the "printed but the
        // prompt doesn't come back" hang) by feeding it an Enter, then detach.
        // SAFETY: `release_parent_console` writes to the borrowed console's
        // input buffer; `FreeConsole` detaches us. Neither has preconditions.
        unsafe {
            release_parent_console();
            free_console();
        }
    }
    std::process::exit(code);
}

/// Console binary's entry (`vantage-box-cli.exe …`).
///
/// A dedicated console-subsystem binary: the host (PowerShell/cmd) attaches a
/// real console and *waits* for it, so stdout/stderr work natively, output
/// lands in the right place, and the prompt returns on its own — no
/// AttachConsole, no Enter injection, no hang. A leading `cli` token is
/// tolerated (muscle memory from `vantage-box cli …`).
pub fn run_console() -> i32 {
    let mut rest: Vec<String> = std::env::args().skip(1).collect();
    if rest.first().is_some_and(|a| a == FLAG) {
        rest.remove(0);
    }
    let code = execute("vantage-box-cli", rest);
    std::process::exit(code);
}

#[derive(Parser)]
#[command(
    name = "vantage-box cli",
    about = "Control Vantage Box from the command line (requires the app to be running)"
)]
struct Cli {
    /// Emit machine-readable JSON to stdout (the integration contract).
    #[arg(long, global = true)]
    json: bool,

    /// For start/stop/toggle/restart: poll `status` until the target state is
    /// reached instead of returning the moment the call is accepted.
    #[arg(long, global = true)]
    wait: bool,

    /// Seconds to wait when `--wait` is set before giving up (exit 5).
    #[arg(long, global = true, default_value_t = 30)]
    timeout: u64,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show combined run + connection status.
    Status,
    /// Start sing-box (service or process, depending on the config).
    Start,
    /// Stop sing-box.
    Stop,
    /// Start if stopped, stop if running.
    Toggle,
    /// Soft restart: snapshot selections, restart, reapply.
    Restart,
    /// List selector/urltest groups and their current selection.
    Proxies,
    /// Select a node in a group.
    Select { group: String, name: String },
    /// Measure latency (ms) of a single node.
    TestDelay { name: String },
    /// Measure latency (ms) of every node in a group.
    TestGroupDelay { group: String },
    /// Show active connections.
    Connections,
    /// Close one connection by id.
    CloseConnection { id: String },
    /// Close all connections.
    CloseAllConnections,
    /// Re-pull subscriptions and re-inject nodes.
    Refresh {
        /// Re-fetch even when the remote content is unchanged.
        #[arg(long)]
        force: bool,
    },
    /// Show subscription state.
    Subs,
    /// Bring the main window to the front.
    Show,
}

/// What can go wrong on a single call: the bus dropped us, or the server
/// returned a JSON-RPC error object.
enum CallError {
    Transport(io::Error),
    Rpc(RpcError),
}

/// A one-shot pipe client: one request, one response (notifications skipped).
struct CliClient {
    write: tokio::io::WriteHalf<tokio::net::windows::named_pipe::NamedPipeClient>,
    read: BufReader<tokio::io::ReadHalf<tokio::net::windows::named_pipe::NamedPipeClient>>,
    id: u64,
}

impl CliClient {
    async fn connect() -> io::Result<Self> {
        // `ClientOptions::open` connects synchronously and fails fast with
        // ERROR_FILE_NOT_FOUND when the pipe (the app) is not running — that
        // maps to exit 3 in the caller. No timeout needed.
        let pipe = ClientOptions::new().open(PIPE_NAME)?;
        let (read, write) = tokio::io::split(pipe);
        Ok(Self {
            write,
            read: BufReader::new(read),
            id: 0,
        })
    }

    /// Send one request and read until the matching response arrives. Any
    /// server→client notifications sitting in the stream are skipped — the CLI
    /// is one-shot and does not subscribe to events.
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        self.id += 1;
        let id = json!(self.id);
        let req = Request {
            id: Some(id.clone()),
            method: method.to_string(),
            params,
        };
        let mut line = serde_json::to_string(&req).map_err(io_err)?;
        line.push('\n');
        self.write
            .write_all(line.as_bytes())
            .await
            .map_err(CallError::Transport)?;

        let mut buf = String::new();
        loop {
            buf.clear();
            let n = self
                .read
                .read_line(&mut buf)
                .await
                .map_err(CallError::Transport)?;
            if n == 0 {
                return Err(CallError::Transport(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the control bus closed the connection",
                )));
            }
            let trimmed = buf.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            let val: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue, // not a JSON line we understand
            };
            // Our response carries our id plus exactly one of result/error.
            if val.get("id") == Some(&id) {
                if let Some(err) = val.get("error") {
                    let err = serde_json::from_value::<RpcError>(err.clone()).map_err(io_err)?;
                    return Err(CallError::Rpc(err));
                }
                if let Some(res) = val.get("result") {
                    return Ok(res.clone());
                }
            }
            // A notification or a response to a different (concurrent) id — skip.
        }
    }
}

async fn run_cli(cli: Cli) -> i32 {
    let mut client = match CliClient::connect().await {
        Ok(c) => c,
        Err(e) => {
            if cli.json {
                println!(
                    "{}",
                    json!({ "code": -32000, "message": format!("control bus unavailable: {e}"), "hint": "is the vantage-box app running?" })
                );
            } else {
                eprintln!(
                    "vantage-box cli: control bus unavailable — {e}\n  (is the vantage-box app running? the bus lives only while it does.)"
                );
            }
            return EXIT_BUS;
        }
    };

    let result = match &cli.command {
        Cmd::Status => client.call("status", json!({})).await,
        Cmd::Start => client.call("start", json!({})).await,
        Cmd::Stop => client.call("stop", json!({})).await,
        Cmd::Toggle => client.call("toggle", json!({})).await,
        Cmd::Restart => client.call("restart", json!({})).await,
        Cmd::Proxies => client.call("proxies", json!({})).await,
        Cmd::Select { group, name } => {
            client
                .call("select", json!({ "group": group, "name": name }))
                .await
        }
        Cmd::TestDelay { name } => client.call("testDelay", json!({ "name": name })).await,
        Cmd::TestGroupDelay { group } => {
            client
                .call("testGroupDelay", json!({ "group": group }))
                .await
        }
        Cmd::Connections => client.call("connections", json!({})).await,
        Cmd::CloseConnection { id } => client.call("closeConnection", json!({ "id": id })).await,
        Cmd::CloseAllConnections => client.call("closeAllConnections", json!({})).await,
        Cmd::Refresh { force } => {
            client
                .call("subscriptions.refresh", json!({ "force": force }))
                .await
        }
        Cmd::Subs => client.call("subscriptions.state", json!({})).await,
        Cmd::Show => client.call("ui.showMain", json!({})).await,
    };

    match result {
        Ok(value) => {
            print_result(&cli.command, &value, cli.json);
            if cli.wait {
                let target = match &cli.command {
                    Cmd::Start | Cmd::Restart => Some(true),
                    Cmd::Stop => Some(false),
                    // Toggle: wait for whatever state the call reports it flipped to.
                    Cmd::Toggle => value["running"].as_bool(),
                    _ => None,
                };
                if let Some(want_running) = target {
                    return wait_for_state(&mut client, want_running, cli.timeout).await;
                }
            }
            EXIT_OK
        }
        Err(CallError::Rpc(err)) => {
            if cli.json {
                // Full error object to stdout — the machine contract.
                println!(
                    "{}",
                    serde_json::to_string(&err).unwrap_or_else(|_| "null".into())
                );
            } else {
                eprintln!("vantage-box cli: {}", err.message);
            }
            if err.code == UNAUTHORIZED {
                EXIT_UNAUTHORIZED
            } else {
                EXIT_ERROR
            }
        }
        Err(CallError::Transport(e)) => {
            if cli.json {
                println!(
                    "{}",
                    json!({ "code": -32000, "message": format!("bus error: {e}") })
                );
            } else {
                eprintln!("vantage-box cli: bus error — {e}");
            }
            EXIT_BUS
        }
    }
}

/// Poll `status` every 400 ms until `run.running == want_running` or the
/// deadline. Returns exit 5 on timeout, 3 if the bus drops mid-poll.
async fn wait_for_state(client: &mut CliClient, want_running: bool, timeout_sec: u64) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(timeout_sec);
    loop {
        tokio::time::sleep(Duration::from_millis(400)).await;
        match client.call("status", json!({})).await {
            Ok(v) => {
                let running = v["run"]["running"].as_bool().unwrap_or(false);
                if running == want_running {
                    return EXIT_OK;
                }
            }
            // A transient RPC error (e.g. the API is mid-restart) is expected
            // right after `restart` — keep polling until the deadline.
            Err(CallError::Rpc(_)) => {}
            Err(CallError::Transport(_)) => return EXIT_BUS,
        }
        if Instant::now() >= deadline {
            return EXIT_WAIT_TIMEOUT;
        }
    }
}

// -- Output ---------------------------------------------------------------------

fn print_result(cmd: &Cmd, value: &Value, json: bool) {
    if json {
        // Compact JSON to stdout is the integration contract.
        println!(
            "{}",
            serde_json::to_string(value).unwrap_or_else(|_| "null".into())
        );
        return;
    }
    match cmd {
        Cmd::Status => print_status(value),
        Cmd::Start | Cmd::Stop | Cmd::Toggle => print_runstatus(value),
        Cmd::Restart => print_restart(value),
        Cmd::Proxies => print_proxies(value),
        Cmd::Select { group, name } => println!("selected: {group} → {name}"),
        Cmd::TestDelay { name } => println!("{name}: {} ms", value.as_u64().unwrap_or(0)),
        Cmd::TestGroupDelay { group } => print_group_delay(group, value),
        Cmd::Connections => print_connections(value),
        Cmd::CloseConnection { id } => println!("closed: {id}"),
        Cmd::CloseAllConnections => println!("closed all connections"),
        Cmd::Refresh { .. } => print_refresh(value),
        Cmd::Subs => print_subs(value),
        Cmd::Show => println!("shown"),
    }
}

fn print_status(value: &Value) {
    let run = &value["run"];
    let conn = &value["connection"];
    println!(
        "mode={} running={} tun={} conn={}",
        run["mode"].as_str().unwrap_or("?"),
        run["running"].as_bool().unwrap_or(false),
        run["tun"].as_bool().unwrap_or(false),
        conn["state"].as_str().unwrap_or("?"),
    );
    if let Some(ver) = conn["version"].as_str() {
        println!("  sing-box: {ver}");
    }
    if let Some(err) = conn["error"].as_str() {
        println!("  error: {err}");
    }
}

fn print_runstatus(value: &Value) {
    println!(
        "mode={} running={} tun={}",
        value["mode"].as_str().unwrap_or("?"),
        value["running"].as_bool().unwrap_or(false),
        value["tun"].as_bool().unwrap_or(false),
    );
}

fn print_restart(value: &Value) {
    println!(
        "restarted: restored={} skipped={} apiBack={}",
        value["restored"].as_array().map(Vec::len).unwrap_or(0),
        value["skipped"].as_array().map(Vec::len).unwrap_or(0),
        value["apiBack"].as_bool().unwrap_or(false),
    );
}

fn print_proxies(value: &Value) {
    let Some(groups) = value["groups"].as_array() else {
        println!("(no groups)");
        return;
    };
    for g in groups {
        let now = g["now"].as_str().unwrap_or("-");
        println!(
            "{} ({}) → {}",
            g["name"].as_str().unwrap_or("?"),
            g["kind"].as_str().unwrap_or("?"),
            now,
        );
    }
}

fn print_group_delay(group: &str, value: &Value) {
    println!("{group}:");
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            println!("  {k}: {} ms", v.as_u64().unwrap_or(0));
        }
    }
}

fn print_connections(value: &Value) {
    let n = value["connections"].as_array().map(Vec::len).unwrap_or(0);
    let dl = value["downloadTotal"].as_u64().unwrap_or(0);
    let ul = value["uploadTotal"].as_u64().unwrap_or(0);
    println!("connections: {n}  ↓{dl}  ↑{ul}");
}

fn print_refresh(value: &Value) {
    println!(
        "refreshed: changed={} restarted={}",
        value["changed"].as_bool().unwrap_or(false),
        value["restarted"].as_bool().unwrap_or(false),
    );
}

fn print_subs(value: &Value) {
    let entries = value["entries"].as_object().map(|m| m.len()).unwrap_or(0);
    let pending = value["applyPending"].as_bool().unwrap_or(false);
    println!("subscriptions: {entries} source(s)  applyPending={pending}");
}

fn io_err(e: impl Display) -> CallError {
    CallError::Transport(io::Error::other(e.to_string()))
}
