#!/usr/bin/env node
/**
 * Генератор иконок приложения из Figma-экспорта.
 *
 * Источник — zip-архив (или папка) с SVG по матрице bg/<empty> × on/off:
 *   - без bg — дефолтный стиль, прозрачный фон. off — дефолт, on — активное
 *              состояние туннеля. off идёт на иконку приложения (bundle: ico/icns,
 *              32/64/128/128@2, Square*Logo, StoreLogo — установщик, окно,
 *              таскбар, список программ), в трей и на favicon. На виндовом
 *              таскбаре без подложки смотрится чище, чем залитый квадрат.
 *   - «bg»   — версии с заливкой и чётко квадратной формой. В бандл не идут
 *              (оставлены в архиве как опция для платформ, требующих opaque-плитку).
 *
 * Что делает скрипт:
 *   1. Из «off» (без подложки) через `tauri icon` — полный набор для бандла
 *      (ico/icns, 32/64/128/128@2, Square*Logo, StoreLogo) в дефолтном виде.
 *   2. Из «off» и «on» — растеризованные tray-off.png / tray-on.png (128px,
 *      crisply downscaled) для динамического состояния трея (src-tauri/src/tray.rs).
 *   3. Из «off» — static/favicon.png (64px), из «off»/«on» — static/logo-off.svg
 *      и logo-on.svg: логотип слева от названия в TitleBar, переключается по
 *      состоянию туннеля (как иконка трея).
 *
 * Запуск:
 *   npm run icons                          # берёт ./vantage-box-icons.zip
 *   npm run icons -- path/to/export.zip
 *   npm run icons -- --src-dir path/to/svg-folder
 *
 * Идемпотентен: безопасно перезапускать после нового экспорта из Figma.
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, cpSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ICONS_DIR = join(ROOT, "src-tauri", "icons");
const STATIC_DIR = join(ROOT, "static");

// ── аргументы ────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
let srcDir = null;
let zipPath = null;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--src-dir") srcDir = resolve(args[++i]);
  else if (!a.startsWith("--")) zipPath = resolve(a);
}
if (!srcDir && !zipPath) zipPath = resolve(ROOT, "vantage-box-icons.zip");

// ── утилиты ──────────────────────────────────────────────────────────────
const log = (m) => console.log(`  • ${m}`);

/** Запуск внешней программы с массивом аргументов (без shell-инъекций). */
function run(bin, args, opts = {}) {
  execFileSync(bin, args, { stdio: opts.silent ? "pipe" : "inherit", ...opts });
}

/** Растеризация SVG в PNG заданного размера через ImageMagick. */
function rasterize(svg, outPng, px) {
  // density = px * 96 / viewBoxUnits (viewBox = 32), затем -resize даёт
  // точный размер. Прозрачный фон, чтобы углы скруглённой плитки не залило.
  const density = String(Math.round((px * 96) / 32));
  run("magick", [
    "-background", "none",
    "-density", density,
    svg,
    "-resize", `${px}x${px}`,
    outPng,
  ], { silent: true });
  log(`magick ${px}×${px} → ${outPng.replace(ROOT + "\\", "")}`);
}

/**
 * Найти SVG в каталоге по матрице меток: имя должно содержать все `includes`
 * и ни одного из `excludes`. Метки — bg/on/off (нижний регистр).
 * Матрица 2×2: bg×<empty>, on×off → четыре варианта, выбираем точно.
 */
function findSvg(dir, includes, excludes = []) {
  const hits = readdirSync(dir)
    .filter((f) => f.toLowerCase().endsWith(".svg"))
    .filter((f) => includes.every((s) => f.toLowerCase().includes(s)))
    .filter((f) => excludes.every((s) => !f.toLowerCase().includes(s)));
  if (hits.length === 0) return null;
  return join(dir, hits.sort((a, b) => a.length - b.length)[0]);
}

// ── 0. подготовка источника ──────────────────────────────────────────────
const work = mkdtempSync(join(tmpdir(), "vb-icons-"));
let svgDir;
if (srcDir) {
  if (!existsSync(srcDir)) throw new Error(`Папка не найдена: ${srcDir}`);
  svgDir = srcDir;
  console.log(`Источник (папка): ${srcDir}`);
} else {
  if (!existsSync(zipPath)) {
    throw new Error(
      `Файл не найден: ${zipPath}\n` +
      `Передайте путь к zip-экспорту из Figma: npm run icons -- path/to/icons.zip`,
    );
  }
  console.log(`Источник (zip): ${zipPath}`);
  // Expand-Archive — штатный распаковщик Windows; не путается в буквах диска.
  run("powershell", [
    "-NoProfile", "-NonInteractive", "-Command",
    `Expand-Archive -LiteralPath '${zipPath}' -DestinationPath '${work}' -Force`,
  ], { silent: true });
  // Если в архиве была папка-обёртка — проваливаемся в неё.
  const entries = readdirSync(work);
  if (entries.length === 1 && statSync(join(work, entries[0])).isDirectory()) {
    svgDir = join(work, entries[0]);
  } else {
    svgDir = work;
  }
}

// Матрица bg/<empty> × on/off. off без bg — дефолт: иконка приложения + трей +
// favicon. bg-варианты в бандл не идут (opaque-плитка на виндовом таскбаре
// выглядит хуже прозрачного). on без bg — активное состояние трея.
const offSvg = findSvg(svgDir, ["off"], ["bg"]) ?? findSvg(svgDir, ["default"]);
const onSvg = findSvg(svgDir, ["on"], ["bg"]);
if (!offSvg) throw new Error("Не найден SVG «off» (имя содержит 'off', но не 'bg').");
if (!onSvg) throw new Error("Не найден SVG «on» (имя содержит 'on', но не 'bg').");
log(`off → ${offSvg.replace(work + "\\", "") || offSvg}`);
log(`on  → ${onSvg.replace(work + "\\", "") || onSvg}`);

// ── 1. полный набор бандла из «off» (без подложки, дефолт) ─────────────────
console.log("\n[1/3] Tauri bundle (off, без подложки):");
const bundleTmp = mkdtempSync(join(tmpdir(), "vb-bundle-"));
run("npx", ["--no-install", "tauri", "icon", offSvg, "-o", bundleTmp]);
mkdirSync(ICONS_DIR, { recursive: true });
// Копируем всё, кроме мобильных платформ (бандл тут только NSIS/Windows).
for (const name of readdirSync(bundleTmp)) {
  if (name === "android" || name === "ios") continue;
  cpSync(join(bundleTmp, name), join(ICONS_DIR, name), { recursive: true });
  log(`${name}${statSync(join(bundleTmp, name)).isDirectory() ? "/" : ""}`);
}

// ── 2. иконки трея (off = дефолт без bg, on = активное состояние) ──────────
console.log("\n[2/3] Tray (off + on, без подложки, 128px):");
rasterize(offSvg, join(ICONS_DIR, "tray-off.png"), 128);
rasterize(onSvg, join(ICONS_DIR, "tray-on.png"), 128);

// ── 3. favicon + логотип в окне (off, без подложки) ────────────────────────
// favicon — растровый (браузеры стабильно берут .png). Логотип слева от
// названия приложения в TitleBar — те же off/on SVG: чётко на любом DPI и
// переключаются по состоянию туннеля, как иконка трея.
console.log("\n[3/3] Favicon + in-app logo (off/on, без подложки):");
mkdirSync(STATIC_DIR, { recursive: true });
rasterize(offSvg, join(STATIC_DIR, "favicon.png"), 64);
cpSync(offSvg, join(STATIC_DIR, "logo-off.svg"));
cpSync(onSvg, join(STATIC_DIR, "logo-on.svg"));
log("logo-off.svg + logo-on.svg → static/");

// ── уборка ───────────────────────────────────────────────────────────────
rmSync(work, { recursive: true, force: true });
rmSync(bundleTmp, { recursive: true, force: true });

console.log("\n✓ Иконки сгенерированы:");
console.log(`    ${ICONS_DIR.replace(ROOT + "\\", "")}  — бандл + tray-off/tray-on`);
console.log(`    ${STATIC_DIR.replace(ROOT + "\\", "")}\\favicon.png + logo-off/on.svg`);
console.log("\n  Трей переключает tray-off ⇄ tray-on в src-tauri/src/tray.rs.");