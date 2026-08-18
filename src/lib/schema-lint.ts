import { linter, type Diagnostic } from '@codemirror/lint';
import type { EditorState } from '@codemirror/state';
import { Draft07, type JsonError } from 'json-schema-library';
import { parseJSON5DocumentState } from 'codemirror-json-schema/json5';
import type { JSONSchema7 } from 'json-schema';
import { m } from '$lib/paraglide/messages.js';

/**
 * Schema linter for the sing-box config editor.
 *
 * Validates the document against **the schema matching the running sing-box version**
 * (see `singbox-schemas.ts`): 1.11/1.12/1.13 — third-party BlackDuty, 1.14+ — official.
 * No need to filter structural noise — the schema simply fits this version's config.
 * When no matching schema exists (1.10.x, unknown version), we pass `null` and the
 * linter stays silent: the real save gate is `sing-box check`.
 *
 * Draft07, not Draft04: the third-party schemas are built for draft 2020-12, and
 * Draft04 crashes on their `oneOf`/`const` with `Cannot read properties of undefined`.
 * The official 1.14 schema validates identically under Draft07 and Draft04
 * (see verify-singbox-schema).
 *
 * A getter instead of a ready-made schema — so the schema can be swapped on the fly
 * when the sing-box version changes, without recreating the editor: CodeEditor
 * updates the variable and calls `forceLinting`.
 */

/** Errors that point at a property's key rather than its value. */
const KEY_ERRORS = new Set([
	'NoAdditionalPropertiesError',
	'RequiredPropertyError',
	'InvalidPropertyNameError',
	'ForbiddenPropertyError',
	'UndefinedValueError'
]);

function errorPath(error: JsonError): string {
	const data = error.data;
	if (data?.pointer && data.pointer !== '#') return data.pointer.slice(1);
	if (data?.property) return `/${data.property}`;
	return '';
}

function rewrite(error: JsonError): string {
	if (error.code === 'type-error') {
		const expected = error.data?.expected;
		const exp = Array.isArray(expected) ? expected.join(` ${m.schema_lint_or()} `) : (expected ?? '');
		const got = error.data?.received ?? '';
		return m.schema_lint_expected({ exp, got }).replace(/\s+$/, '');
	}
	if (error.code === 'one-of-error' || error.code === 'any-of-error') {
		const pointer = error.data?.pointer ?? '';
		return m.schema_lint_no_match({ pointer: pointer ? ` (${pointer})` : '' });
	}
	return (error.message ?? '')
		.replaceAll('in `#` ', '')
		.replaceAll('at `#`', '')
		.replaceAll('/', '.')
		.replaceAll('#.', '')
		.trim();
}

// The draft is heavy (schema is hundreds of KB) and lint runs on every keystroke — cache by schema.
const draftCache = new WeakMap<JSONSchema7, Draft07>();
function getDraft(schema: JSONSchema7): Draft07 {
	let draft = draftCache.get(schema);
	if (!draft) {
		draft = new Draft07(schema as never);
		draftCache.set(schema, draft);
	}
	return draft;
}

/**
 * Pure diagnostics function over EditorState — extracted separately so it can be
 * checked without a DOM (see scripts/verify-singbox-schemas.mjs). `schema === null` means no validation.
 */
export function schemaDiagnostics(state: EditorState, schema: JSONSchema7 | null | undefined): Diagnostic[] {
	if (!schema) return [];
	const text = state.doc.toString();
	if (!text.length) return [];

	const { data, pointers } = parseJSON5DocumentState(state);
	if (data == null) return [];

	let errors: JsonError[] = [];
	try {
		errors = (getDraft(schema).validate(data) as JsonError[]) ?? [];
	} catch {
		// A third-party schema may contain nodes that json-schema-library cannot
		// digest for a particular config — better to skip silently than crash the editor.
		return [];
	}

	const diags: Diagnostic[] = [];
	for (const error of errors) {
		if (!error || typeof error.name !== 'string') continue;

		const path = errorPath(error);
		const message = rewrite(error);

		if (path === '' || error.name === 'MaxPropertiesError' || error.name === 'MinPropertiesError') {
			diags.push({ from: 0, to: 0, message, severity: 'error', source: 'sing-box' });
			continue;
		}

		const pointer = pointers.get(path) as
			| { keyFrom?: number; keyTo?: number; valueFrom?: number; valueTo?: number }
			| undefined;
		if (pointer) {
			const isKey = KEY_ERRORS.has(error.name);
			// A value error underlines valueFrom/valueTo, but for errors on a whole
			// object/array node (AnyOfError on inbounds[0]) those do not exist — only
			// keyFrom/keyTo, covering the whole node. Fall back to those, otherwise the
			// diagnostic is silently lost. ?? , not || : valueFrom can be 0.
			const from = isKey ? pointer.keyFrom : pointer.valueFrom ?? pointer.keyFrom;
			const to = isKey ? pointer.keyTo : pointer.valueTo ?? pointer.keyTo;
			if (from !== undefined && to !== undefined) {
				diags.push({ from, to, message, severity: 'error', source: 'sing-box' });
			}
		} else {
			diags.push({ from: 0, to: 0, message, severity: 'error', source: 'sing-box' });
		}
	}
	return diags;
}

export function schemaLinter(getSchema: () => JSONSchema7 | null | undefined) {
	return linter((view) => schemaDiagnostics(view.state, getSchema()));
}