//! Строит матрицу совместимости Vantage Box с версиями sing-box.
//!
//! Качает несколько релизов с GitHub (тем же загрузчиком, что и приложение),
//! прогоняет по каждому одинаковый набор проб и складывает результат в JSON и
//! Markdown-таблицу. По итогам печатает диапазон, который стоит записать в
//! `SINGBOX_MIN` / `SINGBOX_MAX_EXCLUSIVE`, — так матрица получается из
//! измерений, а не из предположений.
//!
//! Каждая версия проверяется изолированно: свой процесс sing-box, свои порты,
//! конфиг без TUN, своя рабочая папка. Рабочий sing-box пользователя не
//! затрагивается.
//!
//! Запуск: `scripts/compat-matrix.ps1`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use vantage_box_lib::binary;
use vantage_box_lib::clash::client::{parse_version, SINGBOX_MAX_EXCLUSIVE, SINGBOX_MIN};
use vantage_box_lib::compat::{self, ProbeOptions, ProbeReport, CHECK_ORDER};

/// Пауза между версиями: даём портам и файлам освободиться.
const COOLDOWN: Duration = Duration::from_secs(1);

struct Args {
    /// Сколько минорных веток проверять (берём последний патч каждой).
    minors: usize,
    /// Явный список версий; если задан, `minors` игнорируется.
    versions: Vec<String>,
    cache: PathBuf,
    workdir: PathBuf,
    out: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = parse_args();

    println!("Vantage Box — матрица совместимости");
    println!("объявленный диапазон: {}", binary::supported_range());
    println!("кэш загрузок:         {}", args.cache.display());
    println!();

    let targets = match select_versions(&args).await {
        Ok(targets) if !targets.is_empty() => targets,
        Ok(_) => {
            eprintln!("не нашлось ни одной версии со сборкой под эту платформу");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("не удалось получить список релизов: {e}");
            std::process::exit(2);
        }
    };

    println!(
        "проверяю {} версий: {}",
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
                println!("не скачался: {e}");
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
        eprintln!("не удалось записать отчёт: {e}");
        std::process::exit(2);
    }

    println!();
    print_recommendation(&results);
    println!();
    println!("JSON:     {}", args.out.join("compat-matrix.json").display());
    println!("Markdown: {}", args.out.join("compat-matrix.md").display());

    // Ненулевой код только если не прошла ни одна версия: отдельные провалы —
    // это нормальный результат измерения, а не поломка инструмента.
    if results.iter().all(|(_, report)| !report.ok) {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Выбор и загрузка версий
// ---------------------------------------------------------------------------

struct Target {
    version: String,
    asset: String,
    url: String,
}

async fn select_versions(args: &Args) -> Result<Vec<Target>, String> {
    // Берём с запасом: релизов много, а нам нужны последние патчи по минорам.
    let releases = binary::fetch_releases(60).await.map_err(|e| e.to_string())?;

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

    // Последний патч каждой минорной ветки: внутри минора поведение API
    // не меняется, а гонять все патчи — это часы загрузок.
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

/// Скачивает и распаковывает версию, если её ещё нет в кэше.
async fn ensure_binary(cache: &Path, target: &Target) -> Result<PathBuf, String> {
    let dir = cache.join(&target.version);
    let exe = dir.join(if cfg!(windows) { "sing-box.exe" } else { "sing-box" });
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
// Отчёты
// ---------------------------------------------------------------------------

fn summarize(report: &ProbeReport) -> String {
    let total = report.checks.len();
    let passed = report.checks.iter().filter(|c| c.ok).count();
    if report.ok {
        format!("OK ({passed}/{total})")
    } else {
        format!("провал ({passed}/{total})")
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
    let mut text = String::from("# Матрица совместимости с sing-box\n\n");
    text.push_str(&format!(
        "Объявленный диапазон: `{}`\n\n",
        binary::supported_range()
    ));

    text.push_str("| версия |");
    for name in CHECK_ORDER {
        text.push_str(&format!(" {name} |"));
    }
    text.push_str(" итог |\n|---|");
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
                // Пробы после фатального отказа просто не выполнялись.
                None => "—",
            };
            text.push_str(&format!(" {mark} |"));
        }
        text.push_str(&format!(
            " {} |\n",
            if report.ok { "**OK**" } else { "провал" }
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
        text.push_str("\n## Отказы\n\n");
        text.push_str(&failures.join("\n"));
        text.push('\n');
    }

    text
}

/// Печатает диапазон, выведенный из результатов, и сравнивает с объявленным.
fn print_recommendation(results: &[(String, ProbeReport)]) {
    let passing: Vec<(u32, u32, u32)> = results
        .iter()
        .filter(|(_, report)| report.ok)
        .filter_map(|(version, _)| parse_version(version))
        .collect();

    let Some(min) = passing.iter().min().copied() else {
        println!("Ни одна версия не прошла — диапазон менять не по чему.");
        return;
    };
    let max = passing.iter().max().copied().expect("непустой список");

    // Верхняя граница исключающая: следующий минор после старшей рабочей версии.
    let upper = (max.0, max.1 + 1, 0);

    println!("Проверено успешно: {} … {}", fmt(min), fmt(max));
    println!(
        "Рекомендуемые константы (src-tauri/src/clash/client.rs):\n\
         \x20   SINGBOX_MIN           = ({}, {}, {});\n\
         \x20   SINGBOX_MAX_EXCLUSIVE = ({}, {}, {});",
        min.0, min.1, min.2, upper.0, upper.1, upper.2
    );

    if min == SINGBOX_MIN && upper == SINGBOX_MAX_EXCLUSIVE {
        println!("Объявленный диапазон совпадает с измеренным.");
    } else {
        println!(
            "Объявленный сейчас: {} … <{} — расходится с измерениями.",
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
            other => eprintln!("неизвестный аргумент: {other}"),
        }
    }

    args
}
