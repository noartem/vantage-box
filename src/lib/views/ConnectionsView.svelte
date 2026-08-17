<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { age, destination, outbound, processName, rule, source } from '$lib/connection';
	import { formatBytes, formatDuration } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import type { Connection } from '$lib/types';

	/** Активна ли вкладка. После >5 мин отсутствия возврат сбрасывает список
	 *  наверх — к самым «тяжёлым» соединениям, а не к строке, на которой
	 *  остановились полчаса назад. */
	let { active = true }: { active?: boolean } = $props();

	/** Высота строки. Должна совпадать с --h-row: на ней стоит вся арифметика
	 *  виртуализации, поэтому значение продублировано осознанно. */
	const ROW = 22;
	/** Сколько строк дорисовываем сверху и снизу окна, чтобы прокрутка не мигала. */
	const OVERSCAN = 8;

	type SortKey = 'host' | 'process' | 'outbound' | 'rule' | 'down' | 'up' | 'age';

	let busy = $state<string | null>(null);
	let filter = $state('');
	let sortKey = $state<SortKey>('down');
	let sortDesc = $state(true);

	let scrollTop = $state(0);
	let viewportHeight = $state(0);
	let viewport = $state<HTMLDivElement | null>(null);
	/** Возраст соединения тикает сам: снимок /connections не меняет `start`. */
	let now = $state(Date.now());

	const filtered = $derived.by(() => {
		if (!active) return [];
		const q = filter.trim().toLowerCase();
		if (q === '') return app.connections;
		return app.connections.filter(
			(c) =>
				destination(c).toLowerCase().includes(q) ||
				outbound(c).toLowerCase().includes(q) ||
				rule(c).toLowerCase().includes(q) ||
				processName(c).toLowerCase().includes(q) ||
				source(c).includes(q)
		);
	});

	const sorted = $derived.by(() => {
		if (!active) return [];
		const sign = sortDesc ? -1 : 1;
		const by = {
			host: (c: Connection) => destination(c).toLowerCase(),
			process: (c: Connection) => processName(c).toLowerCase(),
			outbound: (c: Connection) => outbound(c).toLowerCase(),
			rule: (c: Connection) => rule(c).toLowerCase(),
			down: (c: Connection) => c.download,
			up: (c: Connection) => c.upload,
			age: (c: Connection) => age(c, now)
		}[sortKey];
		// Копия, а не sort() на месте: исходный массив принадлежит состоянию приложения.
		return [...filtered].sort((a, b) => {
			const left = by(a);
			const right = by(b);
			if (left === right) return 0;
			return (left < right ? -1 : 1) * sign;
		});
	});

	// Виртуализация: в DOM живёт только видимое окно строк. Без неё тысяча
	// соединений превращалась в тысячу узлов, которые перерисовываются раз в секунду.
	const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
	const visible = $derived(Math.ceil(viewportHeight / ROW) + OVERSCAN * 2);
	const slice = $derived(sorted.slice(first, first + visible));
	const padTop = $derived(first * ROW);
	const padBottom = $derived(Math.max(0, (sorted.length - first - slice.length) * ROW));

	function sortBy(key: SortKey) {
		if (sortKey === key) {
			sortDesc = !sortDesc;
			return;
		}
		sortKey = key;
		// Числовые колонки интереснее по убыванию, текстовые — по алфавиту.
		sortDesc = key === 'down' || key === 'up' || key === 'age';
	}

	async function closeOne(id: string) {
		busy = id;
		try {
			await api.closeConnection(id);
			// Следующий кадр /connections сам приедет — обновлять вручную не нужно.
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
		}
	}

	async function closeAll() {
		busy = 'all';
		try {
			await api.closeAllConnections();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
		}
	}

	$effect(() => {
		// Тикер возраста нужен только на активной вкладке: иначе он каждую секунду
		// перезапускал бы сортировку впустую.
		if (!active) return;
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
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
			if (awayFor > AWAY_RESET_MS && viewport) {
				// Дефолтный режим — список наверх.
				viewport.scrollTop = 0;
				scrollTop = 0;
			}
		} else if (awaySince === null) {
			awaySince = Date.now();
		}
	});

	// WS `/connections` может ещё не прислать первый кадр — подтянем разово.
	$effect(() => {
		if (app.status.state !== 'connected') return;
		api
			.getConnections()
			.then((snap) => {
				app.connections = snap.connections;
				app.connectionTotals = { down: snap.downloadTotal, up: snap.uploadTotal };
			})
			.catch(() => {
				// Поток придёт сам — молча.
			});
	});
</script>

{#snippet th(key: SortKey, label: string, cls = '')}
	<button class="th {cls}" class:sorted={sortKey === key} onclick={() => sortBy(key)}>
		<span class="ell">{label}</span>
		{#if sortKey === key}
			<Icon name={sortDesc ? 'sortDesc' : 'sortAsc'} size={10} />
		{/if}
	</button>
{/snippet}

<div class="page">
	<div class="toolbar">
		<span class="count">{app.connections.length} активных</span>
		<span class="muted mono totals">
			↓ {formatBytes(app.connectionTotals.down)} · ↑ {formatBytes(app.connectionTotals.up)}
		</span>
		<span class="spacer"></span>
		<input
			class="filter"
			type="search"
			bind:value={filter}
			placeholder="хост, процесс, outbound, правило…"
			aria-label="Фильтр соединений"
		/>
		<button
			class="danger"
			disabled={busy !== null || app.connections.length === 0}
			onclick={closeAll}
		>
			{busy === 'all' ? 'Закрываю…' : 'Закрыть все'}
		</button>
	</div>

	{#if !active}
		<!-- вкладка не активна: таблицу не рисуем -->
	{:else if app.status.state !== 'connected'}
		<p class="hint">Нет связи с Clash API — sing-box не запущен.</p>
	{:else if app.connections.length === 0}
		<p class="hint">Активных соединений нет.</p>
	{:else if sorted.length === 0}
		<p class="hint">Ничего не подходит под фильтр.</p>
	{:else}
		<div class="table card">
			<div class="row head">
				{@render th('host', 'Хост')}
				{@render th('process', 'Процесс', 'c-process')}
				<span class="th static c-net">Сеть</span>
				{@render th('outbound', 'Outbound')}
				{@render th('rule', 'Правило', 'c-rule')}
				{@render th('down', '↓', 'right')}
				{@render th('up', '↑', 'right')}
				{@render th('age', 'Время', 'right')}
				<span></span>
			</div>

			<div
				class="viewport bounce"
				bind:this={viewport}
				bind:clientHeight={viewportHeight}
				onscroll={(event) => (scrollTop = event.currentTarget.scrollTop)}
			>
				<div style:height="{padTop}px"></div>

				{#each slice as c (c.id)}
					<div class="row">
						<span class="ell" title="{destination(c)}&#10;источник {source(c)}">
							{destination(c)}
						</span>
						<span class="ell muted c-process" title={c.metadata.processPath || 'процесс неизвестен'}>
							{processName(c)}
						</span>
						<span class="muted c-net">{c.metadata.network}</span>
						<span class="ell mono" title={c.chains.join(' ← ')}>{outbound(c)}</span>
						<span class="ell muted c-rule" title={rule(c)}>{rule(c)}</span>
						<span class="mono right">{formatBytes(c.download)}</span>
						<span class="mono right">{formatBytes(c.upload)}</span>
						<span class="mono right muted">{formatDuration(age(c, now))}</span>
						<button
							class="icon-btn"
							title="Закрыть соединение"
							aria-label="Закрыть соединение"
							disabled={busy !== null}
							onclick={() => closeOne(c.id)}
						>
							<Icon name="close" size={11} />
						</button>
					</div>
				{/each}

				<div style:height="{padBottom}px"></div>
			</div>
		</div>
	{/if}
</div>

<style>
	.page {
		display: grid;
		grid-template-rows: auto 1fr;
		gap: var(--sp-3);
		height: 100%;
		min-height: 0;
	}

	.count {
		font-weight: 600;
	}

	.totals {
		font-size: var(--fs-sm);
	}

	.filter {
		width: 240px;
	}

	.table {
		display: grid;
		grid-template-rows: auto 1fr;
		min-height: 0;
		overflow: hidden;
	}

	.viewport {
		overflow-y: auto;
		overflow-x: hidden;
		min-height: 0;
	}

	.row {
		display: grid;
		grid-template-columns:
			minmax(120px, 2fr) minmax(80px, 1fr) 46px minmax(80px, 1fr)
			minmax(80px, 1.2fr) 62px 62px 46px var(--h-ctl);
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-row);
		padding: 0 var(--sp-2) 0 var(--sp-3);
		font-size: var(--fs-sm);
	}

	.row:not(.head):hover {
		background: var(--surface-alt);
	}

	.head {
		border-bottom: 1px solid var(--border);
		background: var(--surface);
	}

	/* Заголовок колонки — кнопка сортировки, но выглядеть должен подписью. */
	.th {
		display: flex;
		align-items: center;
		gap: var(--sp-1);
		height: 100%;
		padding: 0;
		background: transparent;
		border: none;
		color: var(--text-muted);
		font-size: var(--fs-xs);
		text-transform: uppercase;
		letter-spacing: 0.04em;
		min-width: 0;
	}

	.th:hover:not(:disabled) {
		color: var(--text);
		border: none;
	}

	.th.sorted {
		color: var(--text);
	}

	.th.static {
		cursor: default;
	}

	.right {
		text-align: right;
		justify-content: flex-end;
	}

	/* Узкое окно: колонки уходят по одной, начиная с наименее срочных. */
	@media (max-width: 1000px) {
		.row {
			grid-template-columns:
				minmax(120px, 2fr) minmax(80px, 1fr) 46px minmax(80px, 1fr)
				62px 62px 46px var(--h-ctl);
		}

		.c-rule {
			display: none;
		}
	}

	@media (max-width: 820px) {
		.row {
			grid-template-columns: minmax(120px, 2fr) 46px minmax(80px, 1fr) 62px 62px 46px var(--h-ctl);
		}

		.c-process {
			display: none;
		}
	}

	@media (max-width: 700px) {
		.row {
			grid-template-columns: minmax(120px, 2fr) minmax(80px, 1fr) 62px 62px 46px var(--h-ctl);
		}

		.c-net {
			display: none;
		}
	}
</style>
