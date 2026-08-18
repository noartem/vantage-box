// Updates the vendored per-version sing-box schemas (1.11–1.13).
//
// No official JSON schema exists for sing-box < 1.14, so for the 1.11/1.12/1.13
// minors we pull third-party schemas from BlackDuty/sing-box-schema (Zod →
// JSON-schema, draft 2020-12). Here they are downloaded at pinned tags and
// placed in src/lib/schemas/ so the app and checks don't depend on the network.
//
// The tags are picked manually as the latest stable minors. When a new sing-box
// minor version is released, add an entry to VERSIONS below and re-run the script.
//
// Run: node scripts/update-vendored-schemas.mjs
// Check afterwards: npm run verify:editor

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT_DIR = path.join(ROOT, 'src', 'lib', 'schemas');

// BlackDuty/sing-box-schema repo tag → file name in src/lib/schemas/.
// The file name must match the import in src/lib/singbox-schemas.ts.
const VERSIONS = [
	{ tag: 'v1.11.1', file: 'singbox-1.11.1.json' },
	{ tag: 'v1.12.22', file: 'singbox-1.12.22.json' },
	{ tag: 'v1.13.13', file: 'singbox-1.13.13.json' }
];

const URL = (tag) =>
	`https://raw.githubusercontent.com/BlackDuty/sing-box-schema/${tag}/schema.json`;

let failed = 0;
fs.mkdirSync(OUT_DIR, { recursive: true });

for (const { tag, file } of VERSIONS) {
	const dest = path.join(OUT_DIR, file);
	const res = await fetch(URL(tag));
	if (!res.ok) {
		console.error(`✗ ${tag}: HTTP ${res.status}`);
		failed++;
		continue;
	}
	const text = await res.text();
	let parsed;
	try {
		parsed = JSON.parse(text);
	} catch (e) {
		console.error(`✗ ${tag}: not JSON (${e.message})`);
		failed++;
		continue;
	}
	if (!parsed || typeof parsed !== 'object' || !parsed.$defs) {
		console.error(`✗ ${tag}: doesn't look like a sing-box schema (no $defs)`);
		failed++;
		continue;
	}

	const before = fs.existsSync(dest) ? fs.readFileSync(dest, 'utf8') : '';
	if (before === text) {
		console.log(`= ${tag}: unchanged → ${file}`);
		continue;
	}
	fs.writeFileSync(dest, text, 'utf8');
	console.log(`✓ ${tag}: updated → src/lib/schemas/${file}`);
}

if (failed) {
	console.error(`\n✗ failed: ${failed} of ${VERSIONS.length}\n`);
	process.exit(1);
}
console.log(`\n✓ vendored schemas are fine (${VERSIONS.length}). Check: npm run verify:editor\n`);