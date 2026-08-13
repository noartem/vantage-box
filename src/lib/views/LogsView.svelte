<script lang="ts">
	import { formatTime } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import type { LogEntry } from '$lib/types';

	/** Порядок важности: фильтр показывает выбранный уровень и всё, что выше. */
	const LEVELS = ['trace', 'debug', 'info', 'warn', 'error'] as const;

	let minLevel = $state<(typeof LEVELS)[number]>('trace');
	let query = $state('');
	let copied = $state(false);

	let viewport = $state<HTMLDivElement | null>(null);
	/** Автопрокрутка только пока пользователь сам не отлистал вверх. */
	let stickToBottom = $state(true);

	const needle = $derived(query.trim().toLowerCase());

	const visible = $derived(
		app.logs.filter((entry) => {
			if (rank(entry.level) < rank(minLevel)) return false;
			if (needle && !entry.message.toLowerCase().includes(needle)) return false;
			return true;
		})
	);

	function rank(level: string): number {
		const index = LEVELS.indexOf(level.toLowerCase() as (typeof LEVELS)[number]);
		// Незнакомый уровень не должен пропадать из ленты — считаем его важным.
		return index === -1 ? LEVELS.length : index;
	}

	function onScroll() {
		if (!viewport) return;
		const distance = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
		stickToBottom = distance < 40;
	}

	$effect(() => {
		// Зависимость от visible: перерисовали ленту — доехали до низа.
		visible.length;
		if (stickToBottom && viewport) {
			viewport.scrollTop = viewport.scrollHeight;
		}
	});

	function asText(entries: LogEntry[]): string {
		return entries
			.map((e) => `${formatTime(e.time)} ${e.level.toUpperCase()} ${e.message}`)
			.join('\n');
	}

	async function copy() {
		await navigator.clipboard.writeText(asText(visible));
		copied = true;
		setTimeout(() => (copied = false), 1500);
	}
</script>

<div class="page">
	<div class="toolbar card">
		<label>
			<span class="muted">уровень</span>
			<select bind:value={minLevel}>
				{#each LEVELS as level (level)}
					<option value={level}>{level}</option>
				{/each}
			</select>
		</label>

		<input placeholder="поиск по сообщению" bind:value={query} />

		<button onclick={() => app.setLogsPaused(!app.logsPaused)}>
			{app.logsPaused ? 'Продолжить' : 'Пауза'}
		</button>
		<button onclick={copy} disabled={visible.length === 0}>
			{copied ? 'Скопировано' : 'Копировать'}
		</button>
		<button onclick={() => app.clearLogs()} disabled={app.logs.length === 0}>Очистить</button>
	</div>

	<div class="viewport card" bind:this={viewport} onscroll={onScroll}>
		{#if visible.length === 0}
			<p class="muted empty">
				{app.logs.length === 0
					? 'Логи ещё не приходили. Поток /logs открывается автоматически при связи с sing-box.'
					: 'Под фильтр ничего не подходит.'}
			</p>
		{:else}
			{#each visible as entry (entry.id)}
				<div class="row">
					<span class="time">{formatTime(entry.time)}</span>
					<span class="level" data-level={entry.level.toLowerCase()}>{entry.level}</span>
					<span class="message">{entry.message}</span>
				</div>
			{/each}
		{/if}
	</div>

	<p class="muted footer">
		{visible.length} из {app.logs.length}
		{#if app.logsPaused}· на паузе, новые записи копятся и появятся после продолжения{/if}
	</p>
</div>

<style>
	.page {
		display: grid;
		grid-template-rows: auto 1fr auto;
		gap: 10px;
		height: 100%;
		min-height: 0;
	}

	.toolbar {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px 12px;
	}

	.toolbar label {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.toolbar select {
		width: auto;
	}

	.toolbar input {
		flex: 1;
		min-width: 120px;
	}

	.viewport {
		overflow-y: auto;
		padding: 8px 0;
		font-family: var(--mono);
		font-size: 12px;
		user-select: text;
		min-height: 0;
	}

	.row {
		display: grid;
		grid-template-columns: 88px 44px 1fr;
		gap: 10px;
		padding: 2px 12px;
		align-items: baseline;
	}

	.row:hover {
		background: var(--surface-alt);
	}

	.time,
	.level {
		color: var(--text-muted);
	}

	.level[data-level='warn'] {
		color: var(--fair);
	}

	.level[data-level='error'] {
		color: var(--poor);
	}

	.message {
		white-space: pre-wrap;
		word-break: break-word;
	}

	.empty {
		margin: 0;
		padding: 8px 12px;
		font-family: system-ui, sans-serif;
	}

	.footer {
		margin: 0;
		font-variant-numeric: tabular-nums;
	}
</style>
