// Проверка JSONC-режима редактора без DOM.
//
// Держим отдельно от verify-singbox-schema.mjs: там проверяется схема, здесь — что
// редактор действительно принимает JSONC (комментарии и висячие запятые больше не
// подчёркиваются) и при этом не пропускает то, что JSON5 разрешает, а serde на стороне
// Rust — нет.
//
// EditorState работает без браузера, поэтому jsoncDiagnostics вынесена из linter()
// отдельной функцией и проверяется напрямую.
//
// Запуск: task schema:verify (входит в общую проверку) или node --experimental-strip-types

import { EditorState } from '@codemirror/state';
import { json5 } from 'codemirror-json5';
import { json } from '@codemirror/lang-json';
import { syntaxTree } from '@codemirror/language';
import { jsoncDiagnostics } from '../src/lib/jsonc-lint.ts';

let failed = 0;
const ok = (cond, label, detail = '') => {
	console.log(`${cond ? '  ✓' : '  ✗'} ${label}${detail ? ` — ${detail}` : ''}`);
	if (!cond) failed++;
};

const stateFor = (doc, lang) => EditorState.create({ doc, extensions: [lang] });

/** Число узлов ошибки в дереве разбора — так видно, подчеркнёт редактор текст или нет. */
const parseErrors = (state) => {
	let n = 0;
	syntaxTree(state).iterate({
		enter: (node) => {
			if (node.type.isError) n++;
		}
	});
	return n;
};

// ── 1. JSONC разбирается чисто ───────────────────────────────────────────────
const jsonc = `{
  // выбираем уровень логов
  "log": { "level": "info" },
  /* блочный комментарий */
  "outbounds": [
    { "type": "direct", "tag": "direct" },
  ],
}`;

console.log('\nJSONC в редакторе:');
ok(parseErrors(stateFor(jsonc, json5())) === 0, 'комментарии и висячие запятые разбираются без ошибок');
ok(
	parseErrors(stateFor(jsonc, json())) > 0,
	'тот же текст в старом строгом JSON-режиме давал ошибки',
	'подтверждает, что свап режима и был лечением'
);
ok(jsoncDiagnostics(stateFor(jsonc, json5())).length === 0, 'наш линтер к валидному JSONC не придирается');

// Комментарии должны стать токенами — иначе tags.comment в CodeEditor.svelte не сработает.
const commentTokens = (() => {
	const state = stateFor(jsonc, json5());
	let n = 0;
	syntaxTree(state).iterate({
		enter: (node) => {
			if (node.name === 'LineComment' || node.name === 'BlockComment') n++;
		}
	});
	return n;
})();
ok(commentTokens === 2, 'комментарии распознаны как токены (подсветка курсивом)', `${commentTokens} шт.`);

// ── 2. JSON5 сверх JSONC отлавливается ───────────────────────────────────────
console.log('\nЧто JSON5 разрешает, а serde — нет:');
const CASES = [
	["{ 'log': { 'level': 'info' } }", 'одинарные кавычки'],
	['{ log: { level: "info" } }', 'ключ без кавычек'],
	['{ "mtu": Infinity }', 'Infinity'],
	['{ "mtu": NaN }', 'NaN'],
	['{ "mtu": 0x1F }', 'hex-число'],
	['{ "mtu": +9000 }', 'ведущий плюс'],
	['{ "mtu": .5 }', 'число без нуля']
];
for (const [doc, label] of CASES) {
	const diags = jsoncDiagnostics(stateFor(doc, json5()));
	ok(diags.length > 0, label, diags[0]?.message.slice(0, 58) ?? 'не поймано');
}

// Ложных срабатываний быть не должно.
console.log('\nЛожные срабатывания:');
const clean = '{ "mtu": 9000, "ratio": 1.5, "delta": -1, "exp": 1e3, "off": false, "none": null }';
const cleanDiags = jsoncDiagnostics(stateFor(clean, json5()));
ok(cleanDiags.length === 0, 'обычные числа и литералы не трогаем', cleanDiags.map((d) => d.message).join('; '));

console.log(failed === 0 ? '\n✓ JSONC-режим в порядке\n' : `\n✗ провалено проверок: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);
