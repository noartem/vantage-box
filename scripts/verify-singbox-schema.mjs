// Самопроверка сгенерированной схемы.
//
// Смысл: без union-трансформа (см. gen-singbox-schema.mjs) резолвер json-schema-library
// на узлах inbounds/outbounds/route.rules возвращает голый `oneOf` без `properties` —
// автокомплит и подсказки там просто не появляются. Регрессию такого рода глазами не
// поймать, поэтому проверяем ровно те точки, ради которых схема и переделывалась.
//
// Запуск: task schema:verify  (или npm run schema:verify)

import { Draft07, Draft04 } from 'json-schema-library';
import schema from '../src/lib/singbox-schema.generated.json' with { type: 'json' };

let failed = 0;
const ok = (cond, label, detail = '') => {
	console.log(`${cond ? '  ✓' : '  ✗'} ${label}${detail ? ` — ${detail}` : ''}`);
	if (!cond) failed++;
};

// Конфиг-образец: то, что реально встречается в рабочих конфигах.
const sample = {
	log: { level: 'info' },
	inbounds: [{ type: 'tun', tag: 'tun-in' }],
	outbounds: [
		{ type: 'direct', tag: 'direct' },
		{ type: 'selector', tag: 'proxy', outbounds: ['direct'] }
	],
	route: {
		rules: [{ rule_set: ['geosite-ru'], outbound: 'direct' }],
		rule_set: [{ type: 'remote', tag: 'geosite-ru', format: 'binary', url: 'https://example.com/x.srs' }]
	},
	experimental: { clash_api: { external_controller: '127.0.0.1:9090' } }
};

// ── 1. Резолв в точках, где живут автокомплит и подсказки ────────────────────
// codemirror-json-schema берёт для автокомплита Draft07 (features/completion.js).
console.log('\nРезолв схемы (автокомплит и hover):');
const completion = new Draft07(schema);
const EXPECTED = [
	['#/route/rules/0', 60],
	['#/inbounds/0', 80],
	['#/outbounds/0', 80],
	['#/route/rule_set/0', 5],
	['#/log', 4]
];
for (const [pointer, min] of EXPECTED) {
	let count = 0;
	try {
		const sub = completion.getSchema({ pointer, data: sample });
		count = sub?.properties ? Object.keys(sub.properties).length : 0;
	} catch (err) {
		count = 0;
	}
	ok(count >= min, `${pointer}`, `${count} свойств (нужно ≥${min})`);
}

// ── 2. Подписи на месте ──────────────────────────────────────────────────────
// Исходная жалоба была ровно про это: у route подпись есть, у route.rules[].rule_set нет.
console.log('\nПодписи:');
const described = (pointer, field) => {
	try {
		const sub = completion.getSchema({ pointer, data: sample });
		return sub?.properties?.[field]?.description ?? null;
	} catch {
		return null;
	}
};
const CHECKS = [
	['#/route/rules/0', 'rule_set'],
	['#/route/rules/0', 'outbound'],
	['#/route/rules/0', 'domain_suffix'],
	['#/route/rule_set/0', 'type'],
	['#/route/rule_set/0', 'update_interval'],
	['#/inbounds/0', 'listen_port'],
	['#/inbounds/0', 'listen'],
	['#/inbounds/0', 'tls'],
	['#/outbounds/0', 'detour'],
	['#/outbounds/0', 'server'],
	['#/route', 'final'],
	['#/dns', 'servers'],
	['#/experimental', 'clash_api'],
	['#/log', 'level']
];
for (const [pointer, field] of CHECKS) {
	const text = described(pointer, field);
	ok(Boolean(text), `${pointer} → ${field}`, text ? JSON.stringify(text.split('\n')[0].slice(0, 52)) : 'нет описания');
}

const total = (() => {
	let n = 0;
	const walk = (node) => {
		if (!node || typeof node !== 'object') return;
		if (Array.isArray(node)) return node.forEach(walk);
		if (node.description) n++;
		for (const v of Object.values(node)) if (v && typeof v === 'object') walk(v);
	};
	walk(schema);
	return n;
})();
ok(total > 400, 'описаний в схеме всего', String(total));

// ── 3. Валидация не ослабла ──────────────────────────────────────────────────
// Валидатор в codemirror-json-schema — Draft04 (features/validation.js).
console.log('\nВалидация:');
const validator = new Draft04(schema);
ok(validator.validate(sample).length === 0, 'корректный конфиг — без ошибок', String(validator.validate(sample).length));

const broken = {
	log: { level: 'ерунда' },
	inbounds: [{ type: 'mixed', tag: 'in', listen_port: 'не-число' }],
	outbounds: [{ type: 'direct', tag: 'direct' }],
	такого_ключа_нет: 1
};
const errors = validator.validate(broken);
const codes = errors.map((e) => e.code);
ok(errors.length >= 3, 'битый конфиг — ошибки найдены', `${errors.length}: ${[...new Set(codes)].join(', ')}`);
ok(codes.includes('enum-error'), 'неверный log.level пойман');
ok(codes.includes('no-additional-properties-error'), 'неизвестный ключ верхнего уровня пойман');

// ── 4. Метаданные ────────────────────────────────────────────────────────────
console.log('\nМетаданные:');
const meta = schema['x-vantage-box'];
ok(Boolean(meta?.generated), 'дата генерации проставлена', meta?.generated ?? '');
ok(Boolean(meta?.schemaSource), 'источник схемы указан', meta?.schemaSource ?? '');

console.log(failed === 0 ? '\n✓ схема в порядке\n' : `\n✗ провалено проверок: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);
