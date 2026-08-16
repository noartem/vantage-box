<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import CodeEditor from '$lib/components/CodeEditor.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { app } from '$lib/state.svelte';
	import type { CheckResult } from '$lib/types';

	let content = $state('');
	let saved = $state('');
	let loaded = $state(false);
	let busy = $state<string | null>(null);
	let check = $state<CheckResult | null>(null);
	/** Показывается после успешного сохранения: sing-box читает конфиг только при старте. */
	let needsRestart = $state(false);
	let showOutput = $state(false);

	const path = $derived((app.settings?.singBox.configPath ?? '').trim());
	const dirty = $derived(content !== saved);

	type Notice = {
		tone: 'error' | 'warn' | 'ok';
		text: string;
		action?: { label: string; run: () => void };
	};

	/** Раньше над редактором могли выстроиться четыре баннера сразу и съесть
	 *  треть его высоты. Показываем самое срочное одной строкой. */
	const notice = $derived.by<Notice | null>(() => {
		if (check && !check.ok) {
			return { tone: 'error', text: `Конфиг не прошёл проверку: ${firstLine(check.output)}` };
		}
		if (app.configChangedExternally) {
			return {
				tone: 'warn',
				text: dirty
					? 'Файл изменился вне Vantage Box — несохранённые правки будут потеряны.'
					: 'Файл изменился вне Vantage Box.',
				action: { label: 'Перечитать', run: load }
			};
		}
		if (needsRestart) {
			return {
				tone: 'warn',
				text: 'Конфиг сохранён. sing-box читает его только при запуске — нужен перезапуск.'
			};
		}
		if (check?.ok) {
			return {
				tone: 'ok',
				text: check.available ? 'sing-box check: конфиг корректен.' : `JSON корректен. ${check.output}`
			};
		}
		return null;
	});

	function firstLine(text: string): string {
		return text.split('\n')[0] ?? text;
	}

	async function load() {
		busy = 'load';
		try {
			const text = await api.readSingboxConfig();
			content = text;
			saved = text;
			check = null;
			showOutput = false;
			app.configChangedExternally = null;
			loaded = true;
		} catch (e) {
			pushAlert('error', errorText(e));
			loaded = false;
		} finally {
			busy = null;
		}
	}

	async function validate(): Promise<CheckResult | null> {
		busy = 'check';
		try {
			check = await api.checkSingboxConfig(content);
			return check;
		} catch (e) {
			pushAlert('error', errorText(e));
			return null;
		} finally {
			busy = null;
		}
	}

	async function save() {
		// Проверяем до записи: испорченный config.json ломает следующий запуск
		// sing-box, а откатываться потом придётся руками.
		const result = await validate();
		if (!result || !result.ok) return;

		busy = 'save';
		try {
			await api.writeSingboxConfig(content);
			saved = content;
			needsRestart = true;
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
		}
	}

	async function guard(action: () => Promise<unknown>) {
		try {
			await action();
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	$effect(() => {
		// Перечитываем при смене пути в настройках.
		path;
		if (path) load();
	});
</script>

<div class="page">
	{#if !path}
		<p class="hint">
			Путь к <code class="inline">config.json</code> sing-box не задан. Укажите его в настройках,
			раздел «sing-box».
		</p>
	{:else}
		<!-- Панель строго в одну строку: раньше длинный путь с word-break ломал её
			 на две и двигал редактор вниз. -->
		<div class="toolbar">
			<code class="path ell selectable" title={path}>{path}</code>

			{#if dirty}<span class="chip" data-tone="fair">не сохранено</span>{/if}

			<span class="spacer"></span>

			<button
				class="icon-btn"
				title="Проверить через sing-box check"
				aria-label="Проверить"
				disabled={busy !== null || !loaded}
				onclick={validate}
			>
				<Icon name="check" size={13} />
			</button>
			<button
				class="icon-btn"
				title="Перечитать файл с диска"
				aria-label="Перечитать"
				disabled={busy !== null}
				onclick={load}
			>
				<Icon name="refresh" size={13} />
			</button>
			<button
				class="icon-btn"
				title="Открыть во внешнем редакторе"
				aria-label="Открыть во внешнем редакторе"
				disabled={busy !== null}
				onclick={() => guard(() => openPath(path))}
			>
				<Icon name="external" size={13} />
			</button>
			<button
				class="icon-btn"
				title="Показать в папке"
				aria-label="Показать в папке"
				disabled={busy !== null}
				onclick={() => guard(() => revealItemInDir(path))}
			>
				<Icon name="folder" size={13} />
			</button>
			<button class="primary" onclick={save} disabled={busy !== null || !loaded || !dirty}>
				{busy === 'save' ? 'Сохраняю…' : busy === 'check' ? 'Проверяю…' : 'Сохранить'}
			</button>
		</div>

		{#if notice}
			<div class="notice" data-tone={notice.tone}>
				<Icon name={notice.tone === 'ok' ? 'info' : 'alert'} size={12} />
				<span class="ell selectable" title={notice.text}>{notice.text}</span>
				{#if notice.action}
					<button class="act" onclick={notice.action.run}>{notice.action.label}</button>
				{/if}
				{#if check && !check.ok}
					<button class="act" onclick={() => (showOutput = !showOutput)}>
						{showOutput ? 'Свернуть' : 'Подробно'}
					</button>
				{/if}
			</div>
		{/if}

		{#if showOutput && check && !check.ok}
			<pre class="output selectable bounce">{check.output}</pre>
		{/if}

		{#if loaded}
			<CodeEditor
				value={content}
				onchange={(next) => {
					content = next;
					needsRestart = false;
					check = null;
					showOutput = false;
				}}
				onsave={save}
			/>
		{/if}
	{/if}
</div>

<style>
	/* Flex, а не grid: строка результата то есть, то нет, и привязывать редактор
	   к конкретной строке сетки было бы хрупко. */
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		height: 100%;
		min-height: 0;
	}

	.toolbar {
		flex-wrap: nowrap;
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
		color: var(--text-muted);
		min-width: 0;
	}

	.notice {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-alert);
		padding: 0 var(--sp-3);
		border-radius: var(--radius-ctl);
		font-size: var(--fs-sm);
		color: var(--poor);
		background: color-mix(in srgb, var(--poor) 12%, transparent);
		flex-shrink: 0;
	}

	.notice[data-tone='warn'] {
		color: var(--fair);
		background: color-mix(in srgb, var(--fair) 12%, transparent);
	}

	.notice[data-tone='ok'] {
		color: var(--good);
		background: color-mix(in srgb, var(--good) 12%, transparent);
	}

	.notice .ell {
		flex: 1;
	}

	.act {
		height: 18px;
		padding: 0 var(--sp-3);
		font-size: var(--fs-xs);
		background: transparent;
		border-color: currentcolor;
		color: inherit;
	}

	.output {
		margin: 0;
		max-height: 96px;
		overflow: auto;
		padding: var(--sp-3);
		border: 1px solid var(--border);
		border-radius: var(--radius-ctl);
		background: var(--surface);
		font-family: var(--mono);
		font-size: var(--fs-sm);
		white-space: pre-wrap;
		word-break: break-word;
		flex-shrink: 0;
	}

	/* Только редактор растягивается; всё остальное — по содержимому. */
	.page > :global(.editor) {
		flex: 1;
		min-height: 180px;
	}
</style>
