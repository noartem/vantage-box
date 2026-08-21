//! Integration test against a real sing-box.
//!
//! The probes themselves live in `vantage_box_lib::compat` — the same place the
//! compatibility matrix builder takes them from. Here we only run them and
//! check that everything passed: otherwise the test and the matrix would drift
//! apart over time.
//!
//! Isolation is provided by the probe module: a separate sing-box process,
//! non-standard ports, a config without TUN (so no admin rights and no
//! interference with the network stack), and its own working folder. The
//! user's already-running sing-box keeps running.
//!
//! Run via `scripts/integration-test.ps1` — it sets the environment variables
//! and saves the output. Without them, the test reports what is missing and
//! succeeds: it must not get in the way of a regular `cargo test`.

use std::path::PathBuf;

use vantage_box_lib::compat::{self, ProbeOptions};
use vantage_box_lib::settings::CONFIG_DIR_ENV;

const SINGBOX_ENV: &str = "VANTAGE_BOX_TEST_SINGBOX";

#[tokio::test]
async fn live_singbox_roundtrip() {
    let Some((binary, workdir)) = preconditions() else {
        return;
    };

    println!("sing-box binary: {}", binary.display());
    println!("working folder:  {}", workdir.display());
    println!();

    let report = compat::probe(&ProbeOptions::new(binary, workdir)).await;

    for check in &report.checks {
        println!(
            "[{}] {:<20} {}",
            if check.ok { "OK" } else { "!!" },
            check.name,
            check.detail
        );
    }

    println!();
    println!(
        "version: {} ({:?})",
        report.version.as_deref().unwrap_or("undetermined"),
        report.compatibility
    );

    let failed = report.failed();
    assert!(
        failed.is_empty(),
        "probes failed: {}",
        failed
            .iter()
            .map(|c| format!("{} — {}", c.name, c.detail))
            .collect::<Vec<_>>()
            .join("; ")
    );

    println!("\nALL PASSED");
}

/// The test only runs with an explicitly provided environment: that rules out
/// the scenario where it would accidentally go after the user's working
/// configuration.
fn preconditions() -> Option<(PathBuf, PathBuf)> {
    let binary = std::env::var_os(SINGBOX_ENV).map(PathBuf::from);
    let workdir = std::env::var_os(CONFIG_DIR_ENV).map(PathBuf::from);

    match (binary, workdir) {
        (Some(binary), Some(dir)) if binary.is_file() => Some((binary, dir)),
        (Some(binary), Some(_)) => {
            println!(
                "SKIP: {SINGBOX_ENV} points to a non-existent file: {}",
                binary.display()
            );
            None
        }
        _ => {
            println!(
                "SKIP: the {SINGBOX_ENV} and {CONFIG_DIR_ENV} variables are required.\n\
                 Run the test via scripts/integration-test.ps1 — it sets them."
            );
            None
        }
    }
}
