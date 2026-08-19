// Self-check of the generated schema.
//
// The point: without the union transform (see gen-singbox-schema.mjs) the
// json-schema-library resolver returns a bare `oneOf` with no `properties` at
// inbounds/outbounds/route.rules nodes — hover hints simply don't appear there.
// A regression of that kind can't be caught by eye, so we check exactly the
// points the schema was reworked for.
//
// Run: task schema:verify  (or npm run schema:verify)

import { Draft07, Draft04 } from 'json-schema-library';
import schema from '../src/lib/singbox-schema.generated.json' with { type: 'json' };

let failed = 0;
const ok = (cond, label, detail = '') => {
	console.log(`${cond ? '  ✓' : '  ✗'} ${label}${detail ? ` — ${detail}` : ''}`);
	if (!cond) failed++;
};

// A sample config: what actually shows up in real configs.
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

// ── 1. Resolve at the points where hover hints live ──────────────────────────
// codemirror-json-schema uses Draft07 for hover tooltips (features/hover.js).
console.log('\nSchema resolve (hover):');
const resolver = new Draft07(schema);
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
		const sub = resolver.getSchema({ pointer, data: sample });
		count = sub?.properties ? Object.keys(sub.properties).length : 0;
	} catch (err) {
		count = 0;
	}
	ok(count >= min, `${pointer}`, `${count} properties (need ≥${min})`);
}

// ── 2. Signatures in place ───────────────────────────────────────────────────
// The original complaint was exactly this: route has a signature, route.rules[].rule_set does not.
console.log('\nSignatures:');
const described = (pointer, field) => {
	try {
		const sub = resolver.getSchema({ pointer, data: sample });
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
	ok(Boolean(text), `${pointer} → ${field}`, text ? JSON.stringify(text.split('\n')[0].slice(0, 52)) : 'no description');
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
ok(total > 400, 'descriptions in the schema total', String(total));

// ── 3. Validation not weakened ────────────────────────────────────────────────
// The validator in codemirror-json-schema is Draft04 (features/validation.js).
console.log('\nValidation:');
const validator = new Draft04(schema);
ok(validator.validate(sample).length === 0, 'valid config — no errors', String(validator.validate(sample).length));

const broken = {
	log: { level: 'nonsense' },
	inbounds: [{ type: 'mixed', tag: 'in', listen_port: 'not-a-number' }],
	outbounds: [{ type: 'direct', tag: 'direct' }],
	no_such_key: 1
};
const errors = validator.validate(broken);
const codes = errors.map((e) => e.code);
ok(errors.length >= 3, 'broken config — errors found', `${errors.length}: ${[...new Set(codes)].join(', ')}`);
ok(codes.includes('enum-error'), 'invalid log.level caught');
ok(codes.includes('no-additional-properties-error'), 'unknown top-level key caught');

// ── 4. Metadata ──────────────────────────────────────────────────────────────
console.log('\nMetadata:');
const meta = schema['x-vantage-box'];
ok(Boolean(meta?.generated), 'generation date set', meta?.generated ?? '');
ok(Boolean(meta?.schemaSource), 'schema source indicated', meta?.schemaSource ?? '');

console.log(failed === 0 ? '\n✓ schema is fine\n' : `\n✗ checks failed: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);