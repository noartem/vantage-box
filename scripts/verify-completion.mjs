// Checks the whole point of this exercise: autocomplete and signatures inside route.rules[].
//
// verify-singbox-schema.mjs checks the schema at the resolver level, while here we call the
// very same autocomplete source from codemirror-json-schema that fires on Ctrl+Space.
// No DOM is needed: CompletionContext is built on top of EditorState.
//
// The cursor in the examples is placed where it lands during real typing — inside quotes
// or after a started word. In empty space after a comma CodeMirror offers nothing, and that
// is correct behavior, not a bug.
//
// Run via scripts/run-verify.mjs (npm run verify:editor).

import { EditorState } from '@codemirror/state';
import { CompletionContext } from '@codemirror/autocomplete';
import { json5Schema } from 'codemirror-json-schema/json5';
import { json5Completion } from 'codemirror-json-schema/json5';
import log from 'loglevel';
import { disableErrorLogging } from 'best-effort-json-parser';
import { singboxSchema } from '../src/lib/singbox-schema.ts';

log.setLevel('silent');
// Autocomplete by definition parses unclosed JSON, and the parser complains about it to
// console.error. It captures the console reference at module load, so swapping console is
// useless — we mute it the standard way.
disableErrorLogging();

// codemirror-json-schema returns a completion's hint as a function that renders the
// description into a DOM element (features/completion.js: `info: () => el("div", ...)`).
// A createElement stub is enough to call it and read the resulting text — i.e. to check the
// signature via the exact same path the user sees it through.
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

/** What the editor offers when the cursor sits at the `|` marker. */
async function completeAt(doc) {
	const pos = doc.indexOf('|');
	const state = EditorState.create({ doc: doc.replace('|', ''), extensions });
	const result = await source(new CompletionContext(state, pos, true));
	return result?.options ?? [];
}

const labelsOf = (options) => options.map((o) => String(o.label).replace(/"/g, ''));

/** The hint text the way the editor would render it. */
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

// ── The original complaint: route.rules[] had neither autocomplete nor signatures ──
console.log('\nAutocomplete inside route.rules[]:');
const inRules = await completeAt('{ "route": { "rules": [ { "|" } ] } }');
ok(inRules.length > 20, 'suggestions exist', `${inRules.length} variants`);

const byLabel = new Map(inRules.map((o) => [String(o.label).replace(/"/g, ''), o]));
for (const field of ['rule_set', 'domain_suffix', 'ip_cidr', 'process_name', 'clash_mode', 'outbound']) {
	ok(byLabel.has(field), `${field} is offered`);
}

console.log('\nSignatures in suggestions (what was missing):');
for (const field of ['rule_set', 'domain_suffix', 'outbound', 'ip_is_private']) {
	const info = infoOf(byLabel.get(field));
	ok(Boolean(info), `${field} has a signature`, info ? JSON.stringify(info.split('\n')[0].slice(0, 46)) : 'EMPTY');
}
// The ceiling here is set by SagerNet itself: some 1.14 fields (tls_fragment, sniffer, no_drop,
// network_is_expensive and the like) are not described in the docs at all, so there's no text to
// take. Everything that is realistically edited by hand has a signature — verified by name above.
const described = inRules.filter((o) => infoOf(o)).length;
ok(described > inRules.length * 0.5, 'most suggestions have signatures', `${described} of ${inRules.length}`);

// ── Filtering by word prefix ────────────────────────────────────────────────
console.log('\nFiltering by word prefix:');
const partial = labelsOf(await completeAt('{ "route": { "rules": [ { rule| } ] } }'));
ok(partial.includes('rule_set'), 'typing "rule" offers rule_set', partial.join(', ').slice(0, 60));

// ── Other places in the config ───────────────────────────────────────────────
console.log('\nAutocomplete in other places:');
const PLACES = [
	['config root', '{ "log": {}, "|" }', 'experimental'],
	['inside log', '{ "log": { "|" } }', 'level'],
	['inside inbounds[]', '{ "inbounds": [{ "type": "tun", "|" }] }', 'stack'],
	['inside outbounds[]', '{ "outbounds": [{ "type": "selector", "|" }] }', 'outbounds'],
	['inside route.rule_set[]', '{ "route": { "rule_set": [{ "type": "remote", "|" }] } }', 'url'],
	['inside experimental', '{ "experimental": { "|" } }', 'clash_api']
];
for (const [label, doc, expect] of PLACES) {
	const options = await completeAt(doc);
	ok(labelsOf(options).includes(expect), `${label} offers ${expect}`, `${options.length} variants`);
}

// ── Values from enum ─────────────────────────────────────────────────────────
console.log('\nValue substitution:');
const levels = labelsOf(await completeAt('{ "log": { "level": "|" } }'));
ok(levels.includes('info') && levels.includes('debug'), 'log.level offers levels', levels.join(', ').slice(0, 60));

console.log(failed === 0 ? '\n✓ autocomplete is fine\n' : `\n✗ checks failed: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);