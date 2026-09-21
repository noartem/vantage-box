// Checks the editor's JSON syntax handling without a DOM.
//
// Kept separate from verify-singbox-schema.mjs: that one checks the schema, here we
// check that the editor accepts JSONC (comments are fine), warns on trailing commas
// (valid JSONC, not standard JSON), catches strict-JSON breakage at the exact
// offset, and still catches what JSON5 allows but serde on the Rust side does not.
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
import { stripJsonc } from '../src/lib/jsonc.ts';

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
{
	const diags = jsoncDiagnostics(stateFor(jsonc, json5()));
	const errors = diags.filter((d) => d.severity === 'error');
	const warnings = diags.filter((d) => d.severity === 'warning');
	ok(errors.length === 0, 'comments and trailing commas produce no errors', errors.map((d) => d.message).join('; '));
	ok(warnings.length === 2, 'the two trailing commas are warned about', `${warnings.length} warnings`);
	ok(warnings.every((d) => jsonc.slice(d.from, d.to) === ','), 'warnings sit on the commas');
}

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
	for (const d of diags) {
		if (d.severity === 'warning') ok(false, `${label}: unexpected warning`, d.message);
	}
}

// There must be no false positives.
console.log('\nFalse positives:');
const clean = '{ "mtu": 9000, "ratio": 1.5, "delta": -1, "exp": 1e3, "off": false, "none": null }';
const cleanDiags = jsoncDiagnostics(stateFor(clean, json5()));
ok(cleanDiags.length === 0, 'ordinary numbers and literals are left alone', cleanDiags.map((d) => d.message).join('; '));

// ── 3. Strict JSON is enforced at the exact offset ───────────────────────────
console.log('\nStrict JSON gate:');
{
	// Missing comma between keys: V8 points at the second key.
	const broken = '{ "log": { "level": "info" } "outbounds": [] }';
	const diags = jsoncDiagnostics(stateFor(broken, json5()));
	const errors = diags.filter((d) => d.severity === 'error');
	ok(errors.length === 1, 'a missing comma is caught');
	ok(errors[0]?.from === 29, 'the error points at the second key', `from=${errors[0]?.from}`);
	ok(/Expected ',' or '\}'/.test(errors[0]?.message ?? ''), 'the message keeps the V8 detail', errors[0]?.message);
}
{
	// Trailing commas: warnings only, nothing blocks the save.
	for (const doc of ['{ "log": { "level": "info", } }', '{ "outbounds": [ 1, ] }']) {
		const diags = jsoncDiagnostics(stateFor(doc, json5()));
		const errors = diags.filter((d) => d.severity === 'error');
		const warnings = diags.filter((d) => d.severity === 'warning');
		ok(errors.length === 0 && warnings.length === 1, `one warning, no errors: ${doc}`, JSON.stringify(diags));
		ok(doc.slice(warnings[0]?.from, warnings[0]?.to) === ',', 'the warning sits on the comma');
	}
}
{
	// The JSON5-ism is the root cause: the strict error at the same spot is
	// suppressed in favor of the better message.
	const diags = jsoncDiagnostics(stateFor("{ 'log': {} }", json5()));
	const errors = diags.filter((d) => d.severity === 'error');
	ok(errors.length === 1, 'single-quoted key: exactly one error', JSON.stringify(errors));
	ok(jsonc_single_quote_message(errors[0]?.message), 'it is the JSON5-ism message', errors[0]?.message);
}

// ── 4. stripJsonc mirrors the Rust stripper ──────────────────────────────────
console.log('\nstripJsonc parity with src-tauri/src/jsonc.rs:');
{
	const t1 = '{\n  "a": 1 // hello\n}';
	ok(stripJsonc(t1).length === t1.length && JSON.parse(stripJsonc(t1)).a === 1, 'line comment stripped, length kept');
	const t2 = '{/* note\n   more */ "a": 1}';
	ok(JSON.parse(stripJsonc(t2)).a === 1, 'block comment stripped, newlines kept');
	const t3 = '{"url": "http://x/y", "p": "a/*b*/c"}';
	const v3 = JSON.parse(stripJsonc(t3));
	ok(v3.url === 'http://x/y' && v3.p === 'a/*b*/c', 'comment-like strings are untouched');
	const t4 = '{\n  "a": [1, 2, ],\n}';
	ok(JSON.parse(stripJsonc(t4)).a[1] === 2, 'trailing commas stripped');
	const t5 = '{"s": "он сказал \\"привет\\" // не комментарий"}'; // i18n-allow-non-english
	ok(JSON.parse(stripJsonc(t5)).s.includes('привет'), 'escaped quotes and UTF-8 survive'); // i18n-allow-non-english
}

function jsonc_single_quote_message(message) {
	return typeof message === 'string' && message.includes('JSON5');
}

console.log(failed === 0 ? '\n✓ JSONC mode is fine\n' : `\n✗ checks failed: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);