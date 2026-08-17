// Обновляет вендорные per-version схемы sing-box (1.11–1.13).
//
// Официальной JSON-схемы для sing-box < 1.14 не существует, поэтому для миноров
// 1.11/1.12/1.13 мы тянем сторонние схемы из BlackDuty/sing-box-schema (Zod →
// JSON-schema, draft 2020-12). Здесь они скачиваются под зафиксированными тегами
// и кладутся в src/lib/schemas/, чтобы приложение и проверки не зависели от сети.
//
// Теги выбраны вручную как последние стабильные миноры. При выходе новой минорной
// версии sing-box добавьте запись в VERSIONS ниже и повторно запустите скрипт.
//
// Запуск: node scripts/update-vendored-schemas.mjs
// Проверка afterwards: npm run verify:editor

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT_DIR = path.join(ROOT, 'src', 'lib', 'schemas');

// Тег репозитория BlackDuty/sing-box-schema → имя файла в src/lib/schemas/.
// Имя файла должно совпадать с импортом в src/lib/singbox-schemas.ts.
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
		console.error(`✗ ${tag}: не JSON (${e.message})`);
		failed++;
		continue;
	}
	if (!parsed || typeof parsed !== 'object' || !parsed.$defs) {
		console.error(`✗ ${tag}: выглядит не как схема sing-box (нет $defs)`);
		failed++;
		continue;
	}

	const before = fs.existsSync(dest) ? fs.readFileSync(dest, 'utf8') : '';
	if (before === text) {
		console.log(`= ${tag}: без изменений → ${file}`);
		continue;
	}
	fs.writeFileSync(dest, text, 'utf8');
	console.log(`✓ ${tag}: обновлено → src/lib/schemas/${file}`);
}

if (failed) {
	console.error(`\n✗ провалено: ${failed} из ${VERSIONS.length}\n`);
	process.exit(1);
}
console.log(`\n✓ вендорные схемы в порядке (${VERSIONS.length}). Проверь: npm run verify:editor\n`);