import { syntaxTree } from '@codemirror/language';
import { linter, type Diagnostic } from '@codemirror/lint';
import type { EditorState } from '@codemirror/state';
import { m } from '$lib/paraglide/messages.js';
import { findTrailingCommas, stripJsonc } from '$lib/jsonc';

/**
 * Catches JSON5 constructs that the backend will not accept.
 *
 * The editor runs in JSON5 mode — that is the only way CodeMirror understands
 * comments and trailing commas, i.e. JSONC. But JSON5 permits noticeably more:
 * single quotes, unquoted keys, `Infinity`, hex numbers. Parsing on the Rust side is
 * `strip_jsonc()` (strips only comments and trailing commas) plus `serde_json`,
 * and it rejects all of the above.
 *
 * Without this check the editor would be more lenient than the backend: the
 * highlighting is clean, but saving fails with "invalid JSON". So we flag such
 * constructs up front.
 */

const HEX_OR_SIGNED = /^[+]|^0[xX]|^\.|\.$/;
const NOT_JSON = new Set(['Infinity', '-Infinity', '+Infinity', 'NaN', '-NaN', '+NaN']);

/**
 * Separate from `linter()` so it can be checked without a DOM — an EditorState is
 * enough (see scripts/verify-jsonc-lint.mjs).
 */
export function jsoncDiagnostics(state: EditorState): Diagnostic[] {
	const diagnostics: Diagnostic[] = [];
	const text = (from: number, to: number) => state.doc.sliceString(from, to);

	syntaxTree(state).iterate({
		enter: (node) => {
			switch (node.name) {
				case 'PropertyName': {
					const raw = text(node.from, node.to);
					if (raw.startsWith("'")) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: m.jsonc_single_quote_key()
						});
					} else if (!raw.startsWith('"')) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: m.jsonc_unquoted_key()
						});
					}
					break;
				}
				case 'String': {
					if (text(node.from, node.to).startsWith("'")) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: m.jsonc_single_quote_string()
						});
					}
					break;
				}
				case 'Number': {
					const raw = text(node.from, node.to);
					if (NOT_JSON.has(raw)) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: m.jsonc_invalid_token({ raw })
						});
					} else if (HEX_OR_SIGNED.test(raw)) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: m.jsonc_json5_number({ raw })
						});
					}
					break;
				}
			}
		}
	});

	// Trailing commas are valid JSONC but not standard JSON — warn, don't error.
	const doc = state.doc.toString();
	for (const offset of findTrailingCommas(doc)) {
		diagnostics.push({
			from: offset,
			to: offset + 1,
			severity: 'warning',
			message: m.jsonc_trailing_comma()
		});
	}

	// Strict-JSON gate: what the backend actually parses is stripJsonc() +
	// serde_json, so run the same pipeline here. stripJsonc is length-preserving,
	// therefore V8's "at position N" is a valid editor offset.
	const strict = strictJsonError(stripJsonc(doc));
	if (strict && !diagnostics.some((d) => d.from < strict.to && strict.from < d.to)) {
		diagnostics.push(strict);
	}

	return diagnostics;
}

/**
 * Parses the stripped text and turns a JSON.parse failure into a positioned
 * diagnostic. Returns null when the text parses. V8 (WebView2 and Node) reports
 * "… at position N"; other engines degrade to offset 0 with the full message.
 */
function strictJsonError(stripped: string): Diagnostic | null {
	try {
		JSON.parse(stripped);
		return null;
	} catch (e) {
		const message = e instanceof Error ? e.message : String(e);
		const match = /position (\d+)/.exec(message);
		if (!match) {
			return { from: 0, to: 0, severity: 'error', message: m.json_parse_error({ detail: message }) };
		}
		const from = Number(match[1]);
		const detail = message.replace(/\s*in JSON at position.*$/, '');
		return {
			from,
			to: Math.min(from + 1, stripped.length),
			severity: 'error',
			message: m.json_parse_error({ detail })
		};
	}
}

export const jsoncLinter = linter((view) => jsoncDiagnostics(view.state));
