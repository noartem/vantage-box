#!/usr/bin/env node
// Linter: ensures non-English text lives only in localization files.
//
// Flags any non-ASCII Unicode *letter* (Cyrillic, accented Latin,
// CJK, Arabic, etc.) found outside the allowlist. English typography is not
// letters, so em-dashes (—), ellipses (…), and curly quotes (' ' " ") do not trip
// the check — only actual non-English words do.
//
// Allowlisted (may legitimately contain non-English text):
//   - messages/*.json                       Paraglide locale message files (the localizations)
//   - src/lib/i18n.svelte.ts                native language-name data shown in the selector
//   - src/lib/schemas/singbox-*.json        vendored third-party schemas (upstream Chinese title_zh)
//   - src/lib/singbox-schema.generated.json generated official schema (upstream micro/mu unit symbols)
//
// A line may be exempted individually with the marker comment
// `i18n-allow-non-english` — used for test fixtures that must contain non-ASCII
// (e.g. the JSONC UTF-8 preservation test, the version-parser negative case).
//
// Run: node scripts/check-i18n.mjs   (also wired into `npm run check`)

import { execSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';

const NON_ASCII_LETTER = /[-￿]/gu; // candidate non-ASCII chars; filtered to letters below
const IS_LETTER = /^\p{L}$/u;
const ALLOW_MARKER = /i18n-allow-non-english/;

// Paths (relative to repo root, forward slashes) that may hold non-English text.
const ALLOW_PATHS = [
	/^messages\/[^/]+\.json$/, // Paraglide locale message files
	/^src\/lib\/i18n\.svelte\.ts$/, // native language names (i18n selector data)
	/^src\/lib\/schemas\/singbox-.*\.json$/, // vendored third-party schemas (upstream title_zh)
	/^src\/lib\/singbox-schema\.generated\.json$/ // generated official schema (upstream micro/mu)
];

// Binary or generated file types we never scan as text.
const SKIP_EXT = /\.(png|ico|icns|jpg|jpeg|gif|webp|svg|woff2?|ttf|otf|exe|dll|pdb|zip|gz|tar|lock|toml\.lock)$/i;
const SKIP_FILES = /^(package-lock\.json|pnpm-lock\.yaml|Cargo\.lock|yarn\.lock)$/;

function isAllowlisted(p) {
	return ALLOW_PATHS.some((re) => re.test(p));
}

function isNonAsciiLetter(ch) {
	const cp = ch.codePointAt(0);
	return cp > 0x7f && IS_LETTER.test(ch);
}

const files = execSync('git ls-files', { encoding: 'utf8', maxBuffer: 1024 * 1024 * 64 })
	.split('\n')
	.map((s) => s.trim())
	.filter(Boolean);

const violations = [];
for (const file of files) {
	if (SKIP_EXT.test(file) || SKIP_FILES.test(file) || isAllowlisted(file)) continue;
	let text;
	try {
		text = await readFile(file, 'utf8');
	} catch {
		continue; // binary or unreadable — skip
	}
	const lines = text.split('\n');
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i];
		if (ALLOW_MARKER.test(line)) continue;
		const offenders = [];
		for (const ch of line.match(NON_ASCII_LETTER) ?? []) {
			if (isNonAsciiLetter(ch)) offenders.push(ch);
		}
		if (offenders.length) {
			violations.push({ file, line: i + 1, sample: line.trim().slice(0, 140) });
		}
	}
}

if (violations.length) {
	console.error(`\x1b[31mcheck-i18n: ${violations.length} line(s) with non-English text outside localization files:\x1b[0m`);
	for (const v of violations.slice(0, 200)) {
		console.error(`  ${v.file}:${v.line}: ${v.sample}`);
	}
	if (violations.length > 200) console.error(`  ...and ${violations.length - 200} more.`);
	console.error(`\nNon-English text belongs in messages/*.json. If a line must keep non-English (e.g. a UTF-8 test fixture), append: // i18n-allow-non-english`);
	process.exit(1);
}
console.log('check-i18n: no non-English text outside localization files.');