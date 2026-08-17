<script lang="ts">
	import Icon from '$lib/components/Icon.svelte';
	import { formatTime } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import type { LogEntry } from '$lib/types';

	/** Активна ли вкладка. После >5 мин отсутствия возврат сбрасывает ленту
	 *  к свежему хвосту — иначе пользователь возвращается к позиции, где стоял
	 *  полчаса назад, среди уже устаревших строк. */
	let { active = true }: { active?: boolean } = $props();

	/** Порядок важности: фильтр показывает выбранный уровень и всё, что выше. */
	const LEVELS = ['trace', 'debug', 'info', 'warn', 'error'] as const;

	/** Должна совпадать с --h-row: на ней стоит арифметика виртуализации. */
	const ROW = 22;
	const OVERSCAN = 12;

	let minLevel = $state<(typeof LEVELS)[number]>('trace');
	let query = $state('');
	let copied = $state(false);
	/** Перенос длинных сообщений. Ценой отключения виртуализации: строки
	 *  переменной высоты нельзя разложить по фиксированной сетке. */
	let wrap = $state(false);

	let viewport = $state<HTMLDivElement | null>(null);
	/** Автопрокрутка только пока пользователь сам не отлистал вверх. */
	let stickToBottom = $state(true);
	let scrollTop = $state(0);
	let viewportHeight = $state(0);

	const needle = $derived(query.trim().toLowerCase());

	const visible = $derived(
		active
			? app.logs.filter((entry) => {
					if (rank(entry.level) < rank(minLevel)) return false;
					if (needle && !entry.message.toLowerCase().includes(needle)) return false;
					return true;
				})
			: []
	);

	// В DOM держим только видимое окно: лента упирается в потолок 2000 записей,
	// и раньше все 2000 узлов существовали одновременно.
	const first = $derived(wrap ? 0 : Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
	const count = $derived(
		wrap ? visible.length : Math.ceil(viewportHeight / ROW) + OVERSCAN * 2
	);
	const slice = $derived(visible.slice(first, first + count));
	const padTop = $derived(first * ROW);
	const padBottom = $derived(wrap ? 0 : Math.max(0, (visible.length - first - slice.length) * ROW));

	function rank(level: string): number {
		const index = LEVELS.indexOf(level.toLowerCase() as (typeof LEVELS)[number]);
		// Незнакомый уровень не должен пропадать из ленты — считаем его важным.
		return index === -1 ? LEVELS.length : index;
	}

	function onScroll() {
		if (!viewport) return;
		scrollTop = viewport.scrollTop;
		const distance = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
		stickToBottom = distance < 40;
	}

	$effect(() => {
		// Зависимость от visible: перерисовали ленту — доехали до низа.
		visible.length;
		if (stickToBottom && viewport) {
			viewport.scrollTop = viewport.scrollHeight;
			scrollTop = viewport.scrollTop;
		}
	});

	// -------------------------------------------------------------------------
	// Возврат на вкладку после долгого отсутствия
	// -------------------------------------------------------------------------

	/** Сколько отсутствовали на вкладке. Plain-переменная, а не $state: её запись
	 *  не должна перезапускать этот эффект. */
	const AWAY_RESET_MS = 5 * 60_000;
	let awaySince: number | null = null;

	$effect(() => {
		if (active) {
			const awayFor = awaySince === null ? 0 : Date.now() - awaySince;
			awaySince = null;
			if (awayFor > AWAY_RESET_MS) {
				// Дефолтный режим — свежий хвост внизу. stickToBottom=true
				// запускает автопрокрутку выше.
				stickToBottom = true;
			}
		} else if (awaySince === null) {
			awaySince = Date.now();
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
	<div class="toolbar">
		<select bind:value={minLevel} aria-label="Минимальный уровень">
			{#each LEVELS as level (level)}
				<option value={level}>{level}</option>
			{/each}
		</select>

		<input
			class="grow"
			type="search"
			placeholder="поиск по сообщению"
			aria-label="Поиск по сообщению"
			bind:value={query}
		/>

		<button
			class="icon-btn"
			class:on={app.logsPaused}
			title={app.logsPaused ? 'Продолжить: накопленное появится сразу' : 'Пауза'}
			aria-label={app.logsPaused ? 'Продолжить' : 'Пауза'}
			onclick={() => app.setLogsPaused(!app.logsPaused)}
		>
			<Icon name={app.logsPaused ? 'play' : 'pause'} size={12} fill />
		</button>

		<button
			class="icon-btn"
			class:on={wrap}
			title="Переносить длинные сообщения"
			aria-label="Переносить длинные сообщения"
			onclick={() => (wrap = !wrap)}
		>
			<Icon name="logs" size={13} />
		</button>

		<button
			class="icon-btn"
			title={copied ? 'Скопировано' : 'Скопировать отфильтрованное'}
			aria-label="Копировать"
			disabled={visible.length === 0}
			onclick={copy}
		>
			<Icon name={copied ? 'check' : 'copy'} size={13} />
		</button>

		<button
			class="icon-btn"
			title="Очистить ленту"
			aria-label="Очистить"
			disabled={app.logs.length === 0}
			onclick={() => app.clearLogs()}
		>
			<Icon name="trash" size={13} />
		</button>

		<span class="muted mono counter">
			{visible.length}/{app.logs.length}{app.logsPaused ? ' · пауза' : ''}
		</span>
	</div>

	<div class="viewport card bounce" bind:this={viewport} bind:clientHeight={viewportHeight} onscroll={onScroll}>
		{#if !active}
			<!-- вкладка не активна: строки не рисуем -->
		{:else if visible.length === 0}
			<p class="hint empty">
				{app.logs.length === 0
					? 'Логи ещё не приходили. Поток /logs открывается автоматически при связи с sing-box.'
					: 'Под фильтр ничего не подходит.'}
			</p>
		{:else}
			<div style:height="{padTop}px"></div>

			{#each slice as entry (entry.id)}
				<div class="row" class:wrap>
					<span class="time">{formatTime(entry.time)}</span>
					<span class="lv" data-level={entry.level.toLowerCase()}>{entry.level}</span>
					<span class="message" class:ell={!wrap} title={wrap ? undefined : entry.message}>
						{entry.message}
					</span>
				</div>
			{/each}

			<div style:height="{padBottom}px"></div>
		{/if}
	</div>
</div>

<style>
	.page {
		display: grid;
		grid-template-rows: auto 1fr;
		gap: var(--sp-3);
		height: 100%;
		min-height: 0;
	}

	.toolbar select {
		width: auto;
	}

	/* Счётчик был отдельной строкой сетки — целая строка окна ради одной цифры. */
	.counter {
		font-size: var(--fs-sm);
		white-space: nowrap;
	}

	.viewport {
		overflow-y: auto;
		font-family: var(--mono);
		font-size: var(--fs-sm);
		user-select: text;
		min-height: 0;
	}

	.row {
		display: grid;
		/* 80px — ровно под «15:28:58.494» в моноширинном 11px; при 72px метка
		   времени упиралась в уровень. */
		grid-template-columns: 80px 40px 1fr;
		gap: var(--sp-3);
		align-items: center;
		height: var(--h-row);
		padding: 0 var(--sp-3);
	}

	.row.wrap {
		height: auto;
		align-items: baseline;
		padding: var(--sp-1) var(--sp-3);
	}

	.row:hover {
		background: var(--surface-alt);
	}

	.time {
		color: var(--text-muted);
	}

	.lv {
		color: var(--text-muted);
		font-size: var(--fs-xs);
		text-transform: uppercase;
	}

	.lv[data-level='warn'] {
		color: var(--fair);
	}

	.lv[data-level='error'] {
		color: var(--poor);
	}

	.row.wrap .message {
		white-space: pre-wrap;
		word-break: break-word;
	}

	.empty {
		padding: var(--sp-3) var(--sp-4);
		font-family: system-ui, sans-serif;
	}
</style>
