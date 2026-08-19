<script lang="ts">
	import { onMount } from 'svelte';
	import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
	import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
	import {
		HighlightStyle,
		bracketMatching,
		foldGutter,
		foldKeymap,
		indentOnInput,
		syntaxHighlighting
	} from '@codemirror/language';
	import {
		diagnosticCount,
		forEachDiagnostic,
		forceLinting,
		lintGutter,
		lintKeymap,
		linter,
		type Diagnostic
	} from '@codemirror/lint';
	import { highlightSelectionMatches, searchKeymap } from '@codemirror/search';
	import { EditorState } from '@codemirror/state';
	import {
		EditorView,
		drawSelection,
		highlightActiveLine,
		highlightActiveLineGutter,
		hoverTooltip,
		keymap,
		lineNumbers
	} from '@codemirror/view';
	import { tags } from '@lezer/highlight';
	import type { JSONSchema7 } from 'json-schema';
	import { json5, json5ParseLinter } from 'codemirror-json5';
	import { json5SchemaHover } from 'codemirror-json-schema/json5';
	import { stateExtensions } from 'codemirror-json-schema';
	import { jsoncLinter } from '$lib/jsonc-lint';
	import SchemaLintWorker from '$lib/schema-lint-worker?worker';
	import { autocompleteSchema, lintSchemaForVersion } from '$lib/singbox-schemas';

	/** One diagnostic from the editor, recomputed into line/column for the list. */
	export type EditorDiagnostic = {
		from: number;
		to: number;
		line: number;
		col: number;
		message: string;
		severity: 'error' | 'warning' | 'info';
		/** Source: 'sing-box' — the schema, otherwise the JSON5 linter. */
		source: string | undefined;
	};

	let {
		value,
		onchange,
		onsave,
		ondiagnostics,
		version = null,
		schema = null,
		readOnly = false
	}: {
		value: string;
		/** Editor content changed by the user. Not called in read-only mode. */
		onchange?: (next: string) => void;
		/** Ctrl+S inside the editor — more familiar than reaching for a button. */
		onsave?: () => void;
		/** List of active diagnostics, recomputed as edits and lint progress. */
		ondiagnostics?: (diags: EditorDiagnostic[]) => void;
		/** The running sing-box version — used to pick the linter schema. */
		version?: string | null;
		/** An explicit schema to lint/hover against. When set, it overrides the
		 *  sing-box version lookup — used by the settings editor, which lints
		 *  against settings.schema.json regardless of the sing-box version. */
		schema?: JSONSchema7 | null;
		/** Read-only viewer mode: no linting, no editing — just
		 *  syntax highlighting, line numbers and folding. */
		readOnly?: boolean;
	} = $props();

	let container = $state<HTMLDivElement | null>(null);
	let view: EditorView | null = null;

	/** Current linter schema (changes when the version changes). `null` — no linter. */
	let activeLintSchema: JSONSchema7 | null = null;

	// The schema linter runs in a Web worker (src/lib/schema-lint-worker.ts), so
	// parsing the config and Draft07.validate do not freeze typing on the main
	// thread. The worker is created in onMount and destroyed with the editor.
	let lintWorker: Worker | null = null;
	let lintReqId = 0;
	const lintPending = new Map<number, (diags: Diagnostic[]) => void>();

	function setLintSchema(schema: JSONSchema7 | null) {
		lintWorker?.postMessage({ type: 'setSchema', schema });
	}
	function lintAsync(text: string): Promise<Diagnostic[]> {
		const worker = lintWorker;
		if (!worker) return Promise.resolve([]);
		const id = ++lintReqId;
		return new Promise((resolve) => {
			lintPending.set(id, resolve);
			worker.postMessage({ type: 'lint', id, text });
		});
	}

	// Colors are taken from the app themes, so the editor does not fall out of the look.
	const highlight = HighlightStyle.define([
		{ tag: tags.propertyName, color: 'var(--cm-key)' },
		{ tag: tags.string, color: 'var(--cm-string)' },
		{ tag: tags.number, color: 'var(--cm-number)' },
		{ tag: [tags.bool, tags.null], color: 'var(--cm-atom)' },
		{ tag: tags.comment, color: 'var(--cm-comment)', fontStyle: 'italic' },
		{ tag: tags.invalid, color: 'var(--poor)' }
	]);

	const theme = EditorView.theme({
		'&': {
			backgroundColor: 'var(--surface)',
			color: 'var(--text)',
			height: '100%',
			fontSize: '12px'
		},
		'.cm-content': { fontFamily: 'var(--mono)' },
		'.cm-gutters': {
			backgroundColor: 'var(--surface)',
			color: 'var(--text-muted)',
			border: 'none'
		},
		// Semi-transparent active line: drawSelection draws the selection BELOW the
		// line, and an opaque .cm-activeLine background covered it — so the selection
		// within a single line was not visible. Let the selection layer show through.
		'.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--surface-alt) 45%, transparent)' },
		'.cm-activeLineGutter': { backgroundColor: 'var(--surface-alt)' },
		'.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': {
			backgroundColor: 'color-mix(in srgb, var(--accent) 28%, transparent)'
		},
		'.cm-cursor': { borderLeftColor: 'var(--text)' },
		'.cm-tooltip': {
			backgroundColor: 'var(--surface-alt)',
			border: '1px solid var(--border)',
			color: 'var(--text)'
		}
	});

	onMount(() => {
		if (!container) return;

		// Common to both modes: line numbers, active line, folding, selection,
		// highlighting, theme. JSON5 is a superset of JSONC, so the mode gives
		// comments and trailing commas at once (the backend already handles them
		// — src-tauri/src/jsonc.rs).
		const extensions = [
			lineNumbers(),
			highlightActiveLineGutter(),
			highlightActiveLine(),
			foldGutter(),
			drawSelection(),
			syntaxHighlighting(highlight),
			theme,
			json5()
		];

		if (!readOnly) {
			// Linting runs in a Web worker (schema-lint-worker.ts), so parsing the
			// config and Draft07.validate do not freeze typing on the main thread.
			lintWorker = new SchemaLintWorker();
			lintWorker.onmessage = (event: MessageEvent) => {
				const msg = event.data;
				if (msg?.type !== 'lintResult') return;
				const resolve = lintPending.get(msg.id);
				if (resolve) {
					lintPending.delete(msg.id);
					resolve(msg.diags);
				}
			};

			extensions.push(
				highlightSelectionMatches(),
				history(),
				indentOnInput(),
				bracketMatching(),
				closeBrackets(),
				lintGutter(),
				// Hover tooltips come from the active schema — the sing-box 1.14 schema
				// by default (Russian descriptions, union transform, version-neutral field
				// hints), or an explicit `schema` prop (the settings editor lints against
				// settings.schema.json). Autocomplete was removed: the schema-driven
				// completion source recomputed on every keystroke and froze typing.
				hoverTooltip(json5SchemaHover()),
				stateExtensions(schema ?? autocompleteSchema),
				linter(json5ParseLinter()),
				// The schema linter is asynchronous; validation runs in a Web worker
				// (schema-lint-worker.ts). The schema for the sing-box version is sent
				// there in a separate message in the $effect below. CodeMirror discards
				// stale results itself if the document changed in the meantime.
				linter(async (v) => lintAsync(v.state.doc.toString())),
				// JSON5 allows more than serde will digest on the Rust side.
				jsoncLinter,
				keymap.of([
					{
						key: 'Mod-s',
						preventDefault: true,
						run: () => {
							onsave?.();
							return true;
						}
					},
					...closeBracketsKeymap,
					...defaultKeymap,
					...searchKeymap,
					...historyKeymap,
					...foldKeymap,
					...lintKeymap,
					indentWithTab
				]),
				EditorView.updateListener.of((update) => {
					if (update.docChanged) onchange?.(update.state.doc.toString());
					// Recompute the error list when the document changed or lint
					// completed (its result arrives as a separate transaction).
					const countChanged =
						diagnosticCount(update.state) !== diagnosticCount(update.startState);
					if (update.docChanged || countChanged) emitDiagnostics(update.state);
				})
			);
		} else {
			// Read-only viewer: folding is still useful, editing is not.
			extensions.push(EditorState.readOnly.of(true), keymap.of([...foldKeymap]));
		}

		view = new EditorView({
			parent: container,
			state: EditorState.create({
				doc: value,
				extensions
			})
		});

		return () => {
			lintWorker?.terminate();
			lintWorker = null;
			view?.destroy();
			view = null;
		};
	});

	/** Current diagnostics — for the chip and the error list in ConfigView. */
	let lastDiagSig = '';
	function emitDiagnostics(state: EditorState) {
		if (!ondiagnostics) return;
		const diags: EditorDiagnostic[] = [];
		forEachDiagnostic(state, (d, from, to) => {
			const line = state.doc.lineAt(from);
			diags.push({
				from,
				to,
				line: line.number,
				col: from - line.from + 1,
				message: d.message,
				severity: d.severity === 'error' ? 'error' : d.severity === 'warning' ? 'warning' : 'info',
				source: d.source
			});
		});
		// The signature suppresses redundant calls: cursor movement does not change
		// either the set or the positions of diagnostics, so there is nothing to
		// forward to the parent.
		const first = diags[0]?.from ?? -1;
		const last = diags[diags.length - 1]?.from ?? -1;
		const sig = `${diags.length}:${first}:${last}:${state.doc.length}`;
		if (sig === lastDiagSig) return;
		lastDiagSig = sig;
		ondiagnostics(diags);
	}

	/** Jump to a range and place the cursor there — called from the error list. */
	export function jumpTo(from: number, to: number) {
		if (!view) return;
		view.dispatch({
			selection: { anchor: from, head: to ?? from },
			effects: EditorView.scrollIntoView(from, { y: 'center' })
		});
		view.focus();
	}

	$effect(() => {
		// External update (re-read the file from disk). Our own edits come back
		// here unchanged, so the comparison breaks the loop.
		const next = value;
		if (view && next !== view.state.doc.toString()) {
			view.dispatch({
				changes: { from: 0, to: view.state.doc.length, insert: next }
			});
		}
	});

	$effect(() => {
		// Read-only viewers do not lint — no schema to swap.
		if (readOnly) return;
		// An explicit `schema` prop wins (settings editor); otherwise the sing-box
		// version picks the linter schema. A version change swaps it and forces a
		// re-lint so the errors recompute against the new schema.
		version;
		schema;
		const next = schema ?? lintSchemaForVersion(version);
		if (next === activeLintSchema) return;
		activeLintSchema = next;
		// Send the schema to the worker first — postMessage preserves order, so the
		// forced lint below finds the worker already on the new schema.
		setLintSchema(next);
		if (view) forceLinting(view);
	});
</script>

<div class="editor card" bind:this={container}></div>

<style>
	.editor {
		overflow: hidden;
		min-height: 0;
		height: 100%;
	}

	.editor :global(.cm-editor) {
		height: 100%;
	}

	.editor :global(.cm-editor.cm-focused) {
		outline: none;
	}

	.editor :global(.cm-scroller) {
		overflow: auto;
	}
</style>
