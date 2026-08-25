// Entry point for the dedicated console CLI binary `vantage-box-cli.exe`.
//
// This is a console-subsystem binary (no `windows_subsystem = "windows"`), so
// Windows gives it a real console and the host shell waits for it — stdout/
// stderr work natively and the prompt returns on its own. The actual CLI
// logic lives in `vantage_box_lib::cli` (shared with the GUI `cli` subcommand).

fn main() {
    std::process::exit(vantage_box_lib::run_cli_console());
}