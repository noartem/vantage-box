// Runner for the config editor checks.
//
// The checks import both app code (.ts) and codemirror-*. Running them directly through
// node doesn't work: the packages are built with relative imports without extensions and
// a mix of CJS/ESM — the bundler doesn't care, but node can't resolve it. So we bundle
// them through esbuild exactly the way Vite does when building the app, and run the result.
//
// Run: npm run verify:editor  (or task schema:verify)

import { build } from 'esbuild';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const SUITES = [
	['scripts/verify-singbox-schema.mjs', '1.14 schema: autocomplete, signatures, validation'],
	['scripts/verify-singbox-schemas.mjs', 'per-version 1.11–1.13 schemas + version-based mapping'],
	['scripts/verify-jsonc-lint.mjs', 'editor JSONC mode'],
	['scripts/verify-completion.mjs', 'autocomplete via the real CodeMirror source']
];

const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'vb-verify-'));
let failed = 0;

try {
	for (const [entry, title] of SUITES) {
		console.log(`\n━━ ${title} ━━`);
		const outfile = path.join(outDir, path.basename(entry));
		await build({
			entryPoints: [path.join(ROOT, entry)],
			outfile,
			bundle: true,
			platform: 'node',
			format: 'esm',
			target: 'node22',
			loader: { '.json': 'json' },
			// Some dependencies (yaml inside codemirror-json-schema) stay CJS and
			// dynamically require built-in modules — in ESM output they need their own require.
			banner: {
				js: "import { createRequire as __cr } from 'node:module'; const require = __cr(import.meta.url);"
			},
			logLevel: 'warning'
		});
		const run = spawnSync(process.execPath, [outfile], { stdio: 'inherit', cwd: ROOT });
		if (run.status !== 0) failed++;
	}
} finally {
	fs.rmSync(outDir, { recursive: true, force: true });
}

if (failed) {
	console.error(`\n✗ suites failed: ${failed} of ${SUITES.length}\n`);
	process.exit(1);
}
console.log(`\n✓ all editor checks passed (${SUITES.length} suites)\n`);