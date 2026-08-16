// Запускалка проверок редактора конфига.
//
// Проверки импортируют и код приложения (.ts), и codemirror-*. Напрямую через node это
// не заводится: пакеты собраны с относительными импортами без расширений и с CJS/ESM
// вперемешку — сборщику всё равно, node такое не резолвит. Поэтому прогоняем их через
// esbuild ровно так же, как это делает Vite при сборке приложения, и запускаем результат.
//
// Запуск: npm run verify:editor  (или task schema:verify)

import { build } from 'esbuild';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const SUITES = [
	['scripts/verify-singbox-schema.mjs', 'схема: автокомплит, подписи, валидация'],
	['scripts/verify-jsonc-lint.mjs', 'JSONC-режим редактора'],
	['scripts/verify-completion.mjs', 'автокомплит через реальный источник CodeMirror']
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
			// Часть зависимостей (yaml внутри codemirror-json-schema) остаётся CJS и
			// динамически требует встроенные модули — в ESM-выводе им нужен свой require.
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
	console.error(`\n✗ провалено наборов: ${failed} из ${SUITES.length}\n`);
	process.exit(1);
}
console.log(`\n✓ все проверки редактора прошли (${SUITES.length} набора)\n`);
