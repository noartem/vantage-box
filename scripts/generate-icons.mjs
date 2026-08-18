#!/usr/bin/env node
/**
 * App icon generator from a Figma export.
 *
 * Source — a zip archive (or folder) of SVGs along the bg/<empty> × on/off matrix:
 *   - no bg — default style, transparent background. off — the default, on — the
 *             active tunnel state. off goes to the app icon (bundle: ico/icns,
 *             32/64/128/128@2, Square*Logo, StoreLogo — installer, window,
 *             taskbar, program list), the tray, and the favicon. On the Windows
 *             taskbar the no-bg version looks cleaner than a filled square.
 *   - "bg"  — versions with a fill and a strictly square shape. Not bundled
 *             (left in the archive as an option for platforms that need an opaque tile).
 *
 * What the script does:
 *   1. From "off" (no bg) via `tauri icon` — the full bundle set
 *      (ico/icns, 32/64/128/128@2, Square*Logo, StoreLogo) in the default style.
 *   2. From "off" and "on" — rasterized tray-off.png / tray-on.png (128px,
 *      crisply downscaled) for the dynamic tray state (src-tauri/src/tray.rs).
 *   3. From "off" — static/favicon.png (64px); from "off"/"on" — static/logo-off.svg
 *      and logo-on.svg: the logo to the left of the title in TitleBar, toggled by
 *      tunnel state (like the tray icon).
 *
 * Run:
 *   npm run icons                          # uses ./vantage-box-icons.zip
 *   npm run icons -- path/to/export.zip
 *   npm run icons -- --src-dir path/to/svg-folder
 *
 * Idempotent: safe to re-run after a fresh export from Figma.
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, cpSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ICONS_DIR = join(ROOT, "src-tauri", "icons");
const STATIC_DIR = join(ROOT, "static");

// ── args ───────────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
let srcDir = null;
let zipPath = null;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--src-dir") srcDir = resolve(args[++i]);
  else if (!a.startsWith("--")) zipPath = resolve(a);
}
if (!srcDir && !zipPath) zipPath = resolve(ROOT, "vantage-box-icons.zip");

// ── utilities ──────────────────────────────────────────────────────────────
const log = (m) => console.log(`  • ${m}`);

/** Run an external program with an args array (no shell injection). */
function run(bin, args, opts = {}) {
  execFileSync(bin, args, { stdio: opts.silent ? "pipe" : "inherit", ...opts });
}

/** Rasterize an SVG to a PNG of the given size via ImageMagick. */
function rasterize(svg, outPng, px) {
  // density = px * 96 / viewBoxUnits (viewBox = 32), then -resize gives the
  // exact size. Transparent background so the rounded tile corners aren't filled.
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
 * Find an SVG in a directory by a label matrix: the name must contain all
 * `includes` and none of `excludes`. Labels — bg/on/off (lowercase).
 * A 2×2 matrix: bg×<empty>, on×off → four variants, picked precisely.
 */
function findSvg(dir, includes, excludes = []) {
  const hits = readdirSync(dir)
    .filter((f) => f.toLowerCase().endsWith(".svg"))
    .filter((f) => includes.every((s) => f.toLowerCase().includes(s)))
    .filter((f) => excludes.every((s) => !f.toLowerCase().includes(s)));
  if (hits.length === 0) return null;
  return join(dir, hits.sort((a, b) => a.length - b.length)[0]);
}

// ── 0. prepare the source ──────────────────────────────────────────────────
const work = mkdtempSync(join(tmpdir(), "vb-icons-"));
let svgDir;
if (srcDir) {
  if (!existsSync(srcDir)) throw new Error(`Folder not found: ${srcDir}`);
  svgDir = srcDir;
  console.log(`Source (folder): ${srcDir}`);
} else {
  if (!existsSync(zipPath)) {
    throw new Error(
      `File not found: ${zipPath}\n` +
      `Pass the path to a Figma zip export: npm run icons -- path/to/icons.zip`,
    );
  }
  console.log(`Source (zip): ${zipPath}`);
  // Expand-Archive — the built-in Windows unzipper; doesn't get confused by drive letters.
  run("powershell", [
    "-NoProfile", "-NonInteractive", "-Command",
    `Expand-Archive -LiteralPath '${zipPath}' -DestinationPath '${work}' -Force`,
  ], { silent: true });
  // If the archive had a wrapper folder — descend into it.
  const entries = readdirSync(work);
  if (entries.length === 1 && statSync(join(work, entries[0])).isDirectory()) {
    svgDir = join(work, entries[0]);
  } else {
    svgDir = work;
  }
}

// The bg/<empty> × on/off matrix. off without bg — the default: app icon + tray +
// favicon. bg variants are not bundled (an opaque tile looks worse on the Windows
// taskbar than a transparent one). on without bg — the active tray state.
const offSvg = findSvg(svgDir, ["off"], ["bg"]) ?? findSvg(svgDir, ["default"]);
const onSvg = findSvg(svgDir, ["on"], ["bg"]);
if (!offSvg) throw new Error("SVG \"off\" not found (name contains 'off' but not 'bg').");
if (!onSvg) throw new Error("SVG \"on\" not found (name contains 'on' but not 'bg').");
log(`off → ${offSvg.replace(work + "\\", "") || offSvg}`);
log(`on  → ${onSvg.replace(work + "\\", "") || onSvg}`);

// ── 1. full bundle set from "off" (no bg, default) ───────────────────────────
console.log("\n[1/3] Tauri bundle (off, no bg):");
const bundleTmp = mkdtempSync(join(tmpdir(), "vb-bundle-"));
run("npx", ["--no-install", "tauri", "icon", offSvg, "-o", bundleTmp]);
mkdirSync(ICONS_DIR, { recursive: true });
// Copy everything except mobile platforms (the bundle here is NSIS/Windows only).
for (const name of readdirSync(bundleTmp)) {
  if (name === "android" || name === "ios") continue;
  cpSync(join(bundleTmp, name), join(ICONS_DIR, name), { recursive: true });
  log(`${name}${statSync(join(bundleTmp, name)).isDirectory() ? "/" : ""}`);
}

// ── 2. tray icons (off = default no bg, on = active state) ───────────────────
console.log("\n[2/3] Tray (off + on, no bg, 128px):");
rasterize(offSvg, join(ICONS_DIR, "tray-off.png"), 128);
rasterize(onSvg, join(ICONS_DIR, "tray-on.png"), 128);

// ── 3. favicon + in-app logo (off, no bg) ───────────────────────────────────
// favicon — raster (browsers reliably pick up .png). The logo to the left of the
// app name in TitleBar — the same off/on SVG: crisp at any DPI and toggled by
// tunnel state, like the tray icon.
console.log("\n[3/3] Favicon + in-app logo (off/on, no bg):");
mkdirSync(STATIC_DIR, { recursive: true });
rasterize(offSvg, join(STATIC_DIR, "favicon.png"), 64);
cpSync(offSvg, join(STATIC_DIR, "logo-off.svg"));
cpSync(onSvg, join(STATIC_DIR, "logo-on.svg"));
log("logo-off.svg + logo-on.svg → static/");

// ── cleanup ───────────────────────────────────────────────────────────────
rmSync(work, { recursive: true, force: true });
rmSync(bundleTmp, { recursive: true, force: true });

console.log("\n✓ Icons generated:");
console.log(`    ${ICONS_DIR.replace(ROOT + "\\", "")}  — bundle + tray-off/tray-on`);
console.log(`    ${STATIC_DIR.replace(ROOT + "\\", "")}\\favicon.png + logo-off/on.svg`);
console.log("\n  The tray swaps tray-off ⇄ tray-on in src-tauri/src/tray.rs.");