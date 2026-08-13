//! Интеграционный тест против настоящего sing-box.
//!
//! Сами пробы живут в `vantage_box_lib::compat` — там же, откуда их берёт
//! построитель матрицы совместимости. Здесь только запуск и проверка, что всё
//! прошло: иначе тест и матрица со временем разъехались бы.
//!
//! Изоляция обеспечивается модулем проб: отдельный процесс sing-box,
//! нестандартные порты, конфиг без TUN (значит, без прав администратора и без
//! вмешательства в сетевой стек), своя рабочая папка. Уже запущенный sing-box
//! пользователя продолжает работать.
//!
//! Запускать через `scripts/integration-test.ps1` — он подставляет переменные
//! окружения и сохраняет вывод. Без них тест сообщает, чего не хватает, и
//! завершается успехом: в обычном `cargo test` он не должен мешать.

use std::path::PathBuf;

use vantage_box_lib::compat::{self, ProbeOptions};
use vantage_box_lib::settings::CONFIG_DIR_ENV;

const SINGBOX_ENV: &str = "VANTAGE_BOX_TEST_SINGBOX";

#[tokio::test]
async fn live_singbox_roundtrip() {
    let Some((binary, workdir)) = preconditions() else {
        return;
    };

    println!("бинарник sing-box: {}", binary.display());
    println!("рабочая папка:     {}", workdir.display());
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
        "версия: {} ({:?})",
        report.version.as_deref().unwrap_or("не определена"),
        report.compatibility
    );

    let failed = report.failed();
    assert!(
        failed.is_empty(),
        "пробы не прошли: {}",
        failed
            .iter()
            .map(|c| format!("{} — {}", c.name, c.detail))
            .collect::<Vec<_>>()
            .join("; ")
    );

    println!("\nВСЁ ПРОШЛО");
}

/// Тест работает только с явно переданным окружением: так исключён сценарий,
/// в котором он случайно уедет в рабочую конфигурацию пользователя.
fn preconditions() -> Option<(PathBuf, PathBuf)> {
    let binary = std::env::var_os(SINGBOX_ENV).map(PathBuf::from);
    let workdir = std::env::var_os(CONFIG_DIR_ENV).map(PathBuf::from);

    match (binary, workdir) {
        (Some(binary), Some(dir)) if binary.is_file() => Some((binary, dir)),
        (Some(binary), Some(_)) => {
            println!(
                "ПРОПУСК: {SINGBOX_ENV} указывает на несуществующий файл: {}",
                binary.display()
            );
            None
        }
        _ => {
            println!(
                "ПРОПУСК: нужны переменные {SINGBOX_ENV} и {CONFIG_DIR_ENV}.\n\
                 Запустите тест через scripts/integration-test.ps1 — он их подставит."
            );
            None
        }
    }
}
