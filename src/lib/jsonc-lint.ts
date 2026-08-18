import { syntaxTree } from '@codemirror/language';
import { linter, type Diagnostic } from '@codemirror/lint';
import type { EditorState } from '@codemirror/state';
import { m } from '$lib/paraglide/messages.js';

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

	return diagnostics;
}

export const jsoncLinter = linter((view) => jsoncDiagnostics(view.state));
