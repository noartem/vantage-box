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
	import { json5, json5Language, json5ParseLinter } from 'codemirror-json5';
	import { json5SchemaHover } from 'codemirror-json-schema/json5';
	import { stateExtensions } from 'codemirror-json-schema';
	import { jsoncLinter } from '$lib/jsonc-lint';
	import { jsoncCompletion } from '$lib/double-quote-completion';
	import SchemaLintWorker from '$lib/schema-lint-worker?worker';
	import { autocompleteSchema, lintSchemaForVersion } from '$lib/singbox-schemas';

	/** Одна диагностика из редактора, пересчитанная в строку/колонку для списка. */
	export type EditorDiagnostic = {
		from: number;
		to: number;
		line: number;
		col: number;
		message: string;
		severity: 'error' | 'warning' | 'info';
		/** Источник: 'sing-box' — схема, иначе — JSON5-линтер. */
		source: string | undefined;
	};

	let {
		value,
		onchange,
		onsave,
		ondiagnostics,
		version = null
	}: {
		value: string;
		onchange: (next: string) => void;
		/** Ctrl+S внутри редактора — привычнее, чем тянуться к кнопке. */
		onsave?: () => void;
		/** Список активных диагностиок, пересчитывается по мере правок и линта. */
		ondiagnostics?: (diags: EditorDiagnostic[]) => void;
		/** Версия запущенного sing-box — по ней выбираем схему для линтера. */
		version?: string | null;
	} = $props();

	let container = $state<HTMLDivElement | null>(null);
	let view: EditorView | null = null;

	/** Текущая схема для линтера (меняется при смене версии). `null` — линтера нет. */
	let activeLintSchema: JSONSchema7 | null = null;

	// Схемный линтер выполняется в Web-воркере (src/lib/schema-lint-worker.ts), чтобы
	// разбор конфига и Draft07.validate не морозили набор на главном потоке. Воркер
	// создаётся в onMount и уничтожается вместе с редактором.
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
		// Полупрозрачная активная строка: drawSelection рисует выделение ПОД линией,
		// и непрозрачный фон .cm-activeLine его перекрывал — поэтому выделение в
		// пределах одной строки не было видно. Пропускаем сквозь него слой выделения.
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
					// JSON5 — надмножество JSONC, поэтому режим разом даёт комментарии и
					// висячие запятые (бэкенд их уже умеет — src-tauri/src/jsonc.rs).
					json5(),
					// Автокомплит и hover — всегда от официальной схемы 1.14: у неё русские
					// подписи и union-трансформ, а подсказки по полям версионно-нейтральны.
					json5Language.data.of({ autocomplete: jsoncCompletion() }),
					hoverTooltip(json5SchemaHover()),
					stateExtensions(autocompleteSchema),
					linter(json5ParseLinter()),
					// Схемный линтер — асинхронный, валидация идёт в Web-воркере
					// (schema-lint-worker.ts). Схему под версию sing-box шлём туда
					// отдельным сообщением в $effect ниже. CodeMirror сам отбрасывает
					// устаревшие результаты, если документ успел измениться за это время.
					linter(async (v) => lintAsync(v.state.doc.toString())),
					// JSON5 позволяет больше, чем переварит serde на стороне Rust.
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
						...completionKeymap,
						...lintKeymap,
						indentWithTab
					]),
					EditorView.updateListener.of((update) => {
						if (update.docChanged) onchange(update.state.doc.toString());
						// Пересчитываем список ошибок, когда поменялся документ или
						// отработал линт (его результат приезжает отдельной транзакцией).
						const countChanged =
							diagnosticCount(update.state) !== diagnosticCount(update.startState);
						if (update.docChanged || countChanged) emitDiagnostics(update.state);
					})
				]
			})
		});

		return () => {
			lintWorker?.terminate();
			lintWorker = null;
			view?.destroy();
			view = null;
		};
	});

	/** Текущие диагностики — для чипа и списка ошибок в ConfigView. */
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
		// Сигнатура гасит лишние вызовы: курсорные шевеления не меняют ни состав, ни
		// позиции диагностики, а значит родителю пересылать нечего.
		const first = diags[0]?.from ?? -1;
		const last = diags[diags.length - 1]?.from ?? -1;
		const sig = `${diags.length}:${first}:${last}:${state.doc.length}`;
		if (sig === lastDiagSig) return;
		lastDiagSig = sig;
		ondiagnostics(diags);
	}

	/** Прыгнуть к диапазону и поставить туда курсор — вызывается из списка ошибок. */
	export function jumpTo(from: number, to: number) {
		if (!view) return;
		view.dispatch({
			selection: { anchor: from, head: to ?? from },
			effects: EditorView.scrollIntoView(from, { y: 'center' })
		});
		view.focus();
	}

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

	$effect(() => {
		// Смена версии sing-box → меняем схему линтера и принудительно перезапускаем
		// линт, чтобы ошибки перевычислились против новой схемы.
		version;
		const next = lintSchemaForVersion(version);
		if (next === activeLintSchema) return;
		activeLintSchema = next;
		// Сначала отправляем схему в воркер — postMessage сохраняет порядок, поэтому
		// принудительный линт ниже застаёт воркер уже с новой схемой.
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
