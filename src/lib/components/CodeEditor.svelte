<script lang="ts">
	import { onMount } from 'svelte';
	import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from '@codemirror/autocomplete';
	import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
	import {
		HighlightStyle,
		bracketMatching,
		foldGutter,
		foldKeymap,
		indentOnInput,
		syntaxHighlighting
	} from '@codemirror/language';
	import { lintGutter, lintKeymap } from '@codemirror/lint';
	import { highlightSelectionMatches, searchKeymap } from '@codemirror/search';
	import { EditorState } from '@codemirror/state';
	import {
		EditorView,
		drawSelection,
		highlightActiveLine,
		highlightActiveLineGutter,
		keymap,
		lineNumbers
	} from '@codemirror/view';
	import { tags } from '@lezer/highlight';
	import { jsonSchema } from 'codemirror-json-schema';
	import { singboxSchema } from '$lib/singbox-schema';

	let {
		value,
		onchange,
		onsave
	}: {
		value: string;
		onchange: (next: string) => void;
		/** Ctrl+S внутри редактора — привычнее, чем тянуться к кнопке. */
		onsave?: () => void;
	} = $props();

	let container = $state<HTMLDivElement | null>(null);
	let view: EditorView | null = null;

	// Цвета берём из тем приложения, чтобы редактор не выпадал из оформления.
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
		'.cm-activeLine': { backgroundColor: 'var(--surface-alt)' },
		'.cm-activeLineGutter': { backgroundColor: 'var(--surface-alt)' },
		'.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': {
			backgroundColor: 'var(--accent-soft)'
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

		view = new EditorView({
			parent: container,
			state: EditorState.create({
				doc: value,
				extensions: [
					lineNumbers(),
					highlightActiveLineGutter(),
					highlightActiveLine(),
					highlightSelectionMatches(),
					foldGutter(),
					drawSelection(),
					history(),
					indentOnInput(),
					bracketMatching(),
					closeBrackets(),
					autocompletion(),
					lintGutter(),
					syntaxHighlighting(highlight),
					theme,
					// Даёт JSON-режим, схемный автокомплит, подсказки и линтер разом.
					jsonSchema(singboxSchema),
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
						...completionKeymap,
						...lintKeymap,
						indentWithTab
					]),
					EditorView.updateListener.of((update) => {
						if (update.docChanged) onchange(update.state.doc.toString());
					})
				]
			})
		});

		return () => {
			view?.destroy();
			view = null;
		};
	});

	$effect(() => {
		// Внешнее обновление (перечитали файл с диска). Свои же правки сюда
		// возвращаются без изменений, поэтому сравнение гасит цикл.
		const next = value;
		if (view && next !== view.state.doc.toString()) {
			view.dispatch({
				changes: { from: 0, to: view.state.doc.length, insert: next }
			});
		}
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
