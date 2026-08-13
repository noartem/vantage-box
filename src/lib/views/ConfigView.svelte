<script lang="ts">
	import { openPath } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import CodeEditor from '$lib/components/CodeEditor.svelte';
	import { app } from '$lib/state.svelte';
	import type { CheckResult } from '$lib/types';

	let content = $state('');
	let saved = $state('');
	let loaded = $state(false);
	let busy = $state<string | null>(null);
	let error = $state<string | null>(null);
	let check = $state<CheckResult | null>(null);
	/** Показывается после успешного сохранения: sing-box читает конфиг только при старте. */
	let needsRestart = $state(false);

	const path = $derived((app.settings?.singBox.configPath ?? '').trim());
	const dirty = $derived(content !== saved);

	async function load() {
		busy = 'load';
		error = null;
		try {
			const text = await api.readSingboxConfig();
			content = text;
			saved = text;
			check = null;
			app.configChangedExternally = null;
			loaded = true;
		} catch (e) {
			error = errorText(e);
			loaded = false;
		} finally {
			busy = null;
		}
	}

	async function validate(): Promise<CheckResult | null> {
		busy = 'check';
		error = null;
		try {
			check = await api.checkSingboxConfig(content);
			return check;
		} catch (e) {
			error = errorText(e);
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
			error = errorText(e);
		} finally {
			busy = null;
		}
	}

	async function openExternally() {
		error = null;
		try {
			await openPath(path);
		} catch (e) {
			error = errorText(e);
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
		<p class="muted">
			Путь к <code>config.json</code> sing-box не задан. Укажите его на вкладке «Настройки».
		</p>
	{:else}
		<div class="toolbar card">
			<code class="muted selectable">{path}</code>
			<span class="spacer"></span>
			{#if dirty}<span class="muted">есть несохранённые изменения</span>{/if}
			<button onclick={validate} disabled={busy !== null || !loaded}>
				{busy === 'check' ? 'Проверяю…' : 'Проверить'}
			</button>
			<button class="primary" onclick={save} disabled={busy !== null || !loaded || !dirty}>
				{busy === 'save' ? 'Сохраняю…' : 'Сохранить'}
			</button>
			<button onclick={load} disabled={busy !== null}>Перечитать</button>
			<button onclick={openExternally} disabled={busy !== null}>Открыть</button>
		</div>

		{#if error}
			<div class="banner">{error}</div>
		{/if}

		{#if app.configChangedExternally}
			<div class="banner warn">
				Файл изменился вне Vantage Box.
				<button onclick={load}>Перечитать</button>
				{#if dirty}<span class="muted">— несохранённые правки в редакторе будут потеряны</span>{/if}
			</div>
		{/if}

		{#if needsRestart}
			<div class="banner warn">
				Конфиг сохранён. sing-box читает его только при запуске — перезапустите сервис на вкладке
				«Сервис», чтобы изменения вступили в силу.
			</div>
		{/if}

		{#if check}
			<div class="banner" class:ok={check.ok && check.available} class:warn={check.ok && !check.available}>
				{#if check.ok && check.available}
					<strong>sing-box check: конфиг корректен.</strong>
				{:else if check.ok}
					<strong>JSON корректен.</strong>
					{check.output}
				{:else}
					<strong>Конфиг не прошёл проверку.</strong>
					<pre>{check.output}</pre>
				{/if}
			</div>
		{/if}

		{#if loaded}
			<CodeEditor
				value={content}
				onchange={(next) => {
					content = next;
					needsRestart = false;
				}}
				onsave={save}
			/>
		{/if}
	{/if}
</div>

<style>
	/* Flex, а не grid: число баннеров над редактором меняется, и привязывать
	   его к конкретной строке сетки было бы хрупко. */
	.page {
		display: flex;
		flex-direction: column;
		gap: 10px;
		height: 100%;
		min-height: 0;
	}

	.toolbar {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 10px 12px;
		flex-wrap: wrap;
	}

	.spacer {
		flex: 1;
	}

	code {
		font-family: var(--mono);
		font-size: 12px;
		word-break: break-all;
	}

	pre {
		margin: 6px 0 0;
		font-family: var(--mono);
		font-size: 12px;
		white-space: pre-wrap;
		word-break: break-word;
	}

	/* Только редактор растягивается; всё остальное — по содержимому. */
	.page > :global(.editor) {
		flex: 1;
		min-height: 240px;
	}
</style>
