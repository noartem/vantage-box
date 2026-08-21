//! Builds the Vantage Box compatibility matrix against sing-box versions.
//!
//! Downloads several releases from GitHub (with the same loader the app uses),
//! runs the same set of probes against each, and folds the result into JSON
//! and a Markdown table. At the end it prints the range that should go into
//! `SINGBOX_MIN` / `SINGBOX_MAX_EXCLUSIVE` — so the matrix comes from
//! measurements, not from assumptions.
//!
//! Each version is checked in isolation: its own sing-box process, its own
//! ports, a config without TUN, its own working folder. The user's working
//! sing-box is not touched.
//!
//! Run: `scripts/compat-matrix.ps1`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use vantage_box_lib::binary;
use vantage_box_lib::clash::client::{parse_version, SINGBOX_MAX_EXCLUSIVE, SINGBOX_MIN};
use vantage_box_lib::compat::{self, ProbeOptions, ProbeReport, CHECK_ORDER};

/// A pause between versions: let the ports and files be released.
const COOLDOWN: Duration = Duration::from_secs(1);

struct Args {
    /// How many minor branches to check (we take the latest patch of each).
    minors: usize,
    /// An explicit list of versions; when set, `minors` is ignored.
    versions: Vec<String>,
    cache: PathBuf,
    workdir: PathBuf,
    out: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = parse_args();

    println!("Vantage Box — sing-box compatibility matrix");
    println!("declared range: {}", binary::supported_range());
    println!("download cache: {}", args.cache.display());
    println!();

    let targets = match select_versions(&args).await {
        Ok(targets) if !targets.is_empty() => targets,
        Ok(_) => {
            eprintln!("no version with a build for this platform was found");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("failed to fetch the release list: {e}");
            std::process::exit(2);
        }
    };

    println!(
        "checking {} versions: {}",
        targets.len(),
        targets
            .iter()
            .map(|t| t.version.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();

    let mut results: Vec<(String, ProbeReport)> = Vec::new();

    for target in &targets {
        print!("{:<10} ", target.version);
        let binary_path = match ensure_binary(&args.cache, target).await {
            Ok(path) => path,
            Err(e) => {
                println!("download failed: {e}");
                continue;
            }
        };

        let workdir = args.workdir.join(&target.version);
        let _ = std::fs::remove_dir_all(&workdir);

        let report = compat::probe(&ProbeOptions::new(binary_path, workdir)).await;
        println!("{}", summarize(&report));
        for check in report.failed() {
            println!("{:<10}   ✗ {}: {}", "", check.name, check.detail);
        }

        results.push((target.version.clone(), report));
        tokio::time::sleep(COOLDOWN).await;
    }

    if let Err(e) = write_reports(&args.out, &results) {
        eprintln!("failed to write the report: {e}");
        std::process::exit(2);
    }

    println!();
    print_recommendation(&results);
    println!();
    println!(
        "JSON:     {}",
        args.out.join("compat-matrix.json").display()
    );
    println!("Markdown: {}", args.out.join("compat-matrix.md").display());

    // A non-zero code only if not a single version passed: individual failures
    // are a normal measurement result, not a broken tool.
    if results.iter().all(|(_, report)| !report.ok) {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Selecting and downloading versions
// ---------------------------------------------------------------------------

struct Target {
    version: String,
    asset: String,
    url: String,
}

async fn select_versions(args: &Args) -> Result<Vec<Target>, String> {
    // Take with a margin: there are many releases, but we need the latest patches
    // per minor.
    let releases = binary::fetch_releases(60)
        .await
        .map_err(|e| e.to_string())?;

    let mut available: Vec<Target> = releases
        .into_iter()
        .filter_map(|release| {
            Some(Target {
                asset: release.asset?,
                url: release.asset_url?,
                version: release.version,
            })
        })
        .collect();

    if !args.versions.is_empty() {
        available.retain(|t| args.versions.iter().any(|v| v == &t.version));
        available.sort_by_key(|t| parse_version(&t.version));
        return Ok(available);
    }

    // The latest patch of each minor branch: within a minor the API behavior
    // does not change, and running every patch means hours of downloads.
    let mut latest: BTreeMap<(u32, u32), Target> = BTreeMap::new();
    for target in available {
        let Some((major, minor, patch)) = parse_version(&target.version) else {
            continue;
        };
        let entry = latest.entry((major, minor));
        match entry {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(target);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let current = parse_version(&slot.get().version).map(|v| v.2).unwrap_or(0);
                if patch > current {
                    slot.insert(target);
                }
            }
        }
    }

    let mut selected: Vec<Target> = latest.into_values().collect();
    selected.sort_by_key(|t| parse_version(&t.version));
    if selected.len() > args.minors {
        selected.drain(..selected.len() - args.minors);
    }
    Ok(selected)
}

/// Downloads and unpacks a version if it is not yet in the cache.
async fn ensure_binary(cache: &Path, target: &Target) -> Result<PathBuf, String> {
    let dir = cache.join(&target.version);
    let exe = dir.join(if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    });
    if exe.is_file() {
        return Ok(exe);
    }

    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let archive = dir.join(&target.asset);

    binary::download(&target.url, &archive)
        .await
        .map_err(|e| e.to_string())?;
    binary::extract(&archive, &exe).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&archive);

    Ok(exe)
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

fn summarize(report: &ProbeReport) -> String {
    let total = report.checks.len();
    let passed = report.checks.iter().filter(|c| c.ok).count();
    if report.ok {
        format!("OK ({passed}/{total})")
    } else {
        format!("failed ({passed}/{total})")
    }
}

fn write_reports(out: &PathBuf, results: &[(String, ProbeReport)]) -> std::io::Result<()> {
    std::fs::create_dir_all(out)?;

    let json = serde_json::json!({
        "declaredRange": binary::supported_range(),
        "checkOrder": CHECK_ORDER,
        "results": results
            .iter()
            .map(|(version, report)| serde_json::json!({ "version": version, "report": report }))
            .collect::<Vec<_>>(),
    });
    std::fs::write(
        out.join("compat-matrix.json"),
        serde_json::to_vec_pretty(&json).unwrap_or_default(),
    )?;

    std::fs::write(out.join("compat-matrix.md"), markdown(results))?;
    Ok(())
}

fn markdown(results: &[(String, ProbeReport)]) -> String {
    let mut text = String::from("# sing-box compatibility matrix\n\n");
    text.push_str(&format!(
        "Declared range: `{}`\n\n",
        binary::supported_range()
    ));

    text.push_str("| version |");
    for name in CHECK_ORDER {
        text.push_str(&format!(" {name} |"));
    }
    text.push_str(" result |\n|---|");
    for _ in CHECK_ORDER {
        text.push_str("---|");
    }
    text.push_str("---|\n");

    for (version, report) in results {
        text.push_str(&format!("| `{version}` |"));
        for name in CHECK_ORDER {
            let mark = match report.checks.iter().find(|c| c.name == *name) {
                Some(check) if check.ok => "✓",
                Some(_) => "✗",
                // Probes after a fatal failure simply did not run.
                None => "—",
            };
            text.push_str(&format!(" {mark} |"));
        }
        text.push_str(&format!(
            " {} |\n",
            if report.ok { "**OK**" } else { "failed" }
        ));
    }

    let failures: Vec<String> = results
        .iter()
        .flat_map(|(version, report)| {
            report
                .failed()
                .into_iter()
                .map(move |check| format!("- `{version}` — **{}**: {}", check.name, check.detail))
        })
        .collect();

    if !failures.is_empty() {
        text.push_str("\n## Failures\n\n");
        text.push_str(&failures.join("\n"));
        text.push('\n');
    }

    text
}

/// Prints the range derived from the results and compares it with the declared one.
fn print_recommendation(results: &[(String, ProbeReport)]) {
    let passing: Vec<(u32, u32, u32)> = results
        .iter()
        .filter(|(_, report)| report.ok)
        .filter_map(|(version, _)| parse_version(version))
        .collect();

    let Some(min) = passing.iter().min().copied() else {
        println!("No version passed — nothing to change the range against.");
        return;
    };
    let max = passing.iter().max().copied().expect("non-empty list");

    // The upper bound is exclusive: the next minor after the highest passing version.
    let upper = (max.0, max.1 + 1, 0);

    println!("Passed successfully: {} … {}", fmt(min), fmt(max));
    println!(
        "Recommended constants (src-tauri/src/clash/client.rs):\n\
         \x20   SINGBOX_MIN           = ({}, {}, {});\n\
         \x20   SINGBOX_MAX_EXCLUSIVE = ({}, {}, {});",
        min.0, min.1, min.2, upper.0, upper.1, upper.2
    );

    if min == SINGBOX_MIN && upper == SINGBOX_MAX_EXCLUSIVE {
        println!("The declared range matches the measured one.");
    } else {
        println!(
            "Currently declared: {} … <{} — diverges from the measurements.",
            fmt(SINGBOX_MIN),
            fmt(SINGBOX_MAX_EXCLUSIVE)
        );
    }
}

fn fmt(v: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

// ---------------------------------------------------------------------------

fn parse_args() -> Args {
    let mut args = Args {
        minors: 6,
        versions: Vec::new(),
        cache: std::env::temp_dir().join("vantage-box-compat-cache"),
        workdir: std::env::temp_dir().join("vantage-box-compat-work"),
        out: PathBuf::from("../test-results"),
    };

    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        match flag.as_str() {
            "--minors" => {
                if let Some(value) = raw.next().and_then(|v| v.parse().ok()) {
                    args.minors = value;
                }
            }
            "--versions" => {
                if let Some(value) = raw.next() {
                    args.versions = value
                        .split(',')
                        .map(|v| v.trim().trim_start_matches('v').to_string())
                        .filter(|v| !v.is_empty())
                        .collect();
                }
            }
            "--cache" => {
                if let Some(value) = raw.next() {
                    args.cache = PathBuf::from(value);
                }
            }
            "--workdir" => {
                if let Some(value) = raw.next() {
                    args.workdir = PathBuf::from(value);
                }
            }
            "--out" => {
                if let Some(value) = raw.next() {
                    args.out = PathBuf::from(value);
                }
            }
            "--help" | "-h" => {
                println!(
                    "compat-probe [--minors N] [--versions 1.12.9,1.13.16] \
                     [--cache DIR] [--workdir DIR] [--out DIR]"
                );
                std::process::exit(0);
            }
            other => eprintln!("unknown argument: {other}"),
        }
    }

    args
}
