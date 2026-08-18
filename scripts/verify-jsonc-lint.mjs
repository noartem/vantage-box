// Checks the editor's JSONC mode without a DOM.
//
// Kept separate from verify-singbox-schema.mjs: that one checks the schema, here we
// check that the editor actually accepts JSONC (comments and trailing commas are no
// longer underlined) and still catches what JSON5 allows but serde on the Rust side
// does not.
//
// EditorState works without a browser, so jsoncDiagnostics is factored out of linter()
// as a standalone function and checked directly.
//
// Run: task schema:verify (part of the full check) or node --experimental-strip-types

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

/** Number of error nodes in the parse tree — shows whether the editor underlines the text. */
const parseErrors = (state) => {
	let n = 0;
	syntaxTree(state).iterate({
		enter: (node) => {
			if (node.type.isError) n++;
		}
	});
	return n;
};

// ── 1. JSONC parses cleanly ─────────────────────────────────────────────────
const jsonc = `{
  // pick the log level
  "log": { "level": "info" },
  /* a block comment */
  "outbounds": [
    { "type": "direct", "tag": "direct" },
  ],
}`;

console.log('\nJSONC in the editor:');
ok(parseErrors(stateFor(jsonc, json5())) === 0, 'comments and trailing commas parse without errors');
ok(
	parseErrors(stateFor(jsonc, json())) > 0,
	'the same text in the old strict JSON mode produced errors',
	'confirms that swapping the mode was the fix'
);
ok(jsoncDiagnostics(stateFor(jsonc, json5())).length === 0, 'our linter does not nitpick valid JSONC');

// Comments must become tokens — otherwise tags.comment in CodeEditor.svelte won't fire.
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
ok(commentTokens === 2, 'comments recognized as tokens (italic highlighting)', `${commentTokens} found`);

// ── 2. JSON5 beyond JSONC is caught ──────────────────────────────────────────
console.log('\nWhat JSON5 allows but serde does not:');
const CASES = [
	["{ 'log': { 'level': 'info' } }", 'single quotes'],
	['{ log: { level: "info" } }', 'unquoted key'],
	['{ "mtu": Infinity }', 'Infinity'],
	['{ "mtu": NaN }', 'NaN'],
	['{ "mtu": 0x1F }', 'hex number'],
	['{ "mtu": +9000 }', 'leading plus'],
	['{ "mtu": .5 }', 'number without leading zero']
];
for (const [doc, label] of CASES) {
	const diags = jsoncDiagnostics(stateFor(doc, json5()));
	ok(diags.length > 0, label, diags[0]?.message.slice(0, 58) ?? 'not caught');
}

// There must be no false positives.
console.log('\nFalse positives:');
const clean = '{ "mtu": 9000, "ratio": 1.5, "delta": -1, "exp": 1e3, "off": false, "none": null }';
const cleanDiags = jsoncDiagnostics(stateFor(clean, json5()));
ok(cleanDiags.length === 0, 'ordinary numbers and literals are left alone', cleanDiags.map((d) => d.message).join('; '));

console.log(failed === 0 ? '\n✓ JSONC mode is fine\n' : `\n✗ checks failed: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);