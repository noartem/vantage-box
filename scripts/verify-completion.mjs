// Проверка того, ради чего всё затевалось: автокомплит и подписи внутри route.rules[].
//
// verify-singbox-schema.mjs проверяет схему на уровне резолвера, а здесь дёргается тот
// самый источник автодополнения из codemirror-json-schema, который сработает по Ctrl+Space.
// DOM для этого не нужен: CompletionContext строится поверх EditorState.
//
// Курсор в примерах ставится туда, где он оказывается при реальном наборе — внутрь кавычек
// или после начатого слова. В пустом месте после запятой CodeMirror предложений не даёт,
// и это правильное поведение, а не поломка.
//
// Запускается через scripts/run-verify.mjs (npm run verify:editor).

import { EditorState } from '@codemirror/state';
import { CompletionContext } from '@codemirror/autocomplete';
import { json5Schema } from 'codemirror-json-schema/json5';
import { json5Completion } from 'codemirror-json-schema/json5';
import log from 'loglevel';
import { disableErrorLogging } from 'best-effort-json-parser';
import { singboxSchema } from '../src/lib/singbox-schema.ts';

log.setLevel('silent');
// Автокомплит по определению разбирает незакрытый JSON, и парсер жалуется на это в
// console.error. Ссылку на console он захватывает при загрузке модуля, поэтому подменять
// console бесполезно — глушим штатным способом.
disableErrorLogging();

// Подсказку к предложению codemirror-json-schema отдаёт функцией, которая рендерит
// описание в DOM-элемент (features/completion.js: `info: () => el("div", ...)`).
// Заглушки createElement хватает, чтобы вызвать её и прочитать получившийся текст —
// то есть проверить подпись ровно тем же путём, каким её увидит пользователь.
globalThis.document = {
	createElement: () => ({
		innerHTML: '',
		innerText: '',
		setAttribute() {},
		appendChild() {}
	})
};


let failed = 0;
const ok = (cond, label, detail = '') => {
	console.log(`${cond ? '  ✓' : '  ✗'} ${label}${detail ? ` — ${detail}` : ''}`);
	if (!cond) failed++;
};

const source = json5Completion();
const extensions = json5Schema(singboxSchema);

/** Что предложит редактор, если курсор стоит на месте маркера `|`. */
async function completeAt(doc) {
	const pos = doc.indexOf('|');
	const state = EditorState.create({ doc: doc.replace('|', ''), extensions });
	const result = await source(new CompletionContext(state, pos, true));
	return result?.options ?? [];
}

const labelsOf = (options) => options.map((o) => String(o.label).replace(/"/g, ''));

/** Текст подсказки так, как его отрисует редактор. */
const infoOf = (option) => {
	const raw = option?.info;
	if (typeof raw === 'string') return raw;
	if (typeof raw !== 'function') return '';
	try {
		const html = raw()?.innerHTML ?? '';
		return html.replace(/<[^>]*>/g, '').trim();
	} catch {
		return '';
	}
};

// ── Исходная жалоба: в route.rules[] не было ни автокомплита, ни подписей ────
console.log('\nАвтокомплит внутри route.rules[]:');
const inRules = await completeAt('{ "route": { "rules": [ { "|" } ] } }');
ok(inRules.length > 20, 'предложения есть', `${inRules.length} вариантов`);

const byLabel = new Map(inRules.map((o) => [String(o.label).replace(/"/g, ''), o]));
for (const field of ['rule_set', 'domain_suffix', 'ip_cidr', 'process_name', 'clash_mode', 'outbound']) {
	ok(byLabel.has(field), `предлагается ${field}`);
}

console.log('\nПодписи в предложениях (то, чего не хватало):');
for (const field of ['rule_set', 'domain_suffix', 'outbound', 'ip_is_private']) {
	const info = infoOf(byLabel.get(field));
	ok(Boolean(info), `у ${field} есть подпись`, info ? JSON.stringify(info.split('\n')[0].slice(0, 46)) : 'ПУСТО');
}
// Потолок здесь задаёт сам SagerNet: часть полей 1.14 (tls_fragment, sniffer, no_drop,
// network_is_expensive и подобные) в документации не описана вообще, брать текст неоткуда.
// Всё, что реально правят руками, подписи имеет — это проверено поимённо выше.
const described = inRules.filter((o) => infoOf(o)).length;
ok(described > inRules.length * 0.5, 'подписи у большинства предложений', `${described} из ${inRules.length}`);

// ── Фильтрация по началу слова ───────────────────────────────────────────────
console.log('\nФильтрация по началу слова:');
const partial = labelsOf(await completeAt('{ "route": { "rules": [ { rule| } ] } }'));
ok(partial.includes('rule_set'), 'набранное "rule" предлагает rule_set', partial.join(', ').slice(0, 60));

// ── Остальные места конфига ──────────────────────────────────────────────────
console.log('\nАвтокомплит в других местах:');
const PLACES = [
	['корень конфига', '{ "log": {}, "|" }', 'experimental'],
	['внутри log', '{ "log": { "|" } }', 'level'],
	['внутри inbounds[]', '{ "inbounds": [{ "type": "tun", "|" }] }', 'stack'],
	['внутри outbounds[]', '{ "outbounds": [{ "type": "selector", "|" }] }', 'outbounds'],
	['внутри route.rule_set[]', '{ "route": { "rule_set": [{ "type": "remote", "|" }] } }', 'url'],
	['внутри experimental', '{ "experimental": { "|" } }', 'clash_api']
];
for (const [label, doc, expect] of PLACES) {
	const options = await completeAt(doc);
	ok(labelsOf(options).includes(expect), `${label} предлагает ${expect}`, `${options.length} вариантов`);
}

// ── Значения из enum ─────────────────────────────────────────────────────────
console.log('\nПодстановка значений:');
const levels = labelsOf(await completeAt('{ "log": { "level": "|" } }'));
ok(levels.includes('info') && levels.includes('debug'), 'log.level предлагает уровни', levels.join(', ').slice(0, 60));

console.log(failed === 0 ? '\n✓ автокомплит в порядке\n' : `\n✗ провалено проверок: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);
