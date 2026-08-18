<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { age, destination, outbound, processName, rule, source } from '$lib/connection';
	import { formatBytes, formatDuration } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { Connection } from '$lib/types';

	/** Whether the tab is active. After >5 min away, returning resets the list
	 *  to the top — to the "heaviest" connections, not the row we stopped on
	 *  half an hour ago. */
	let { active = true }: { active?: boolean } = $props();

	/** Row height. Must match --h-row: all the virtualization arithmetic rests on
	 *  it, so the value is intentionally duplicated. */
	const ROW = 22;
	/** How many extra rows we render above and below the viewport so scrolling does not flash. */
	const OVERSCAN = 8;

	type SortKey = 'host' | 'process' | 'outbound' | 'rule' | 'down' | 'up' | 'age';

	let busy = $state<string | null>(null);
	let filter = $state('');
	let sortKey = $state<SortKey>('down');
	let sortDesc = $state(true);

	let scrollTop = $state(0);
	let viewportHeight = $state(0);
	let viewport = $state<HTMLDivElement | null>(null);
	/** Connection age ticks on its own: a /connections snapshot does not change `start`. */
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
		// A copy, not an in-place sort(): the source array belongs to app state.
		return [...filtered].sort((a, b) => {
			const left = by(a);
			const right = by(b);
			if (left === right) return 0;
			return (left < right ? -1 : 1) * sign;
		});
	});

	// Virtualization: only the visible window of rows lives in the DOM. Without it,
	// a thousand connections became a thousand nodes repainted once a second.
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
		// Numeric columns are more interesting descending, text ones alphabetical.
		sortDesc = key === 'down' || key === 'up' || key === 'age';
	}

	async function closeOne(id: string) {
		busy = id;
		try {
			await api.closeConnection(id);
			// The next /connections frame arrives on its own — no manual refresh needed.
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
		// The age ticker is only needed on the active tab: otherwise it would
		// retrigger sorting every second for nothing.
		if (!active) return;
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});

	// -------------------------------------------------------------------------
	// Returning to the tab after a long absence
	// -------------------------------------------------------------------------

	/** How long we were away from the tab. A plain variable, not $state: writing
	 *  it must not retrigger this effect. */
	const AWAY_RESET_MS = 5 * 60_000;
	let awaySince: number | null = null;

	$effect(() => {
		if (active) {
			const awayFor = awaySince === null ? 0 : Date.now() - awaySince;
			awaySince = null;
			if (awayFor > AWAY_RESET_MS && viewport) {
				// Default mode — list at the top.
				viewport.scrollTop = 0;
				scrollTop = 0;
			}
		} else if (awaySince === null) {
			awaySince = Date.now();
		}
	});

	// WS `/connections` may not have sent the first frame yet — pull it once.
	$effect(() => {
		if (app.status.state !== 'connected') return;
		api
			.getConnections()
			.then((snap) => {
				app.connections = snap.connections;
				app.connectionTotals = { down: snap.downloadTotal, up: snap.uploadTotal };
			})
			.catch(() => {
				// The stream will arrive on its own — stay silent.
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
		<span class="count">{m.connections_active_count({ count: app.connections.length })}</span>
		<span class="muted mono totals">
			↓ {formatBytes(app.connectionTotals.down)} · ↑ {formatBytes(app.connectionTotals.up)}
		</span>
		<span class="spacer"></span>
		<input
			class="filter"
			type="search"
			bind:value={filter}
			placeholder={m.connections_filter_placeholder()}
			aria-label={m.connections_filter_label()}
		/>
		<button
			class="danger"
			disabled={busy !== null || app.connections.length === 0}
			onclick={closeAll}
		>
			{busy === 'all' ? m.connections_closing() : m.connections_close_all()}
		</button>
	</div>

	{#if !active}
		<!-- tab inactive: do not render the table -->
	{:else if app.status.state !== 'connected'}
		<p class="hint">{m.connections_no_api()}</p>
	{:else if app.connections.length === 0}
		<p class="hint">{m.connections_none()}</p>
	{:else if sorted.length === 0}
		<p class="hint">{m.common_no_filter_match()}</p>
	{:else}
		<div class="table card">
			<div class="row head">
				{@render th('host', m.connections_col_host())}
				{@render th('process', m.connections_col_process(), 'c-process')}
				<span class="th static c-net">{m.connections_col_network()}</span>
				{@render th('outbound', m.connections_col_outbound())}
				{@render th('rule', m.connections_col_rule(), 'c-rule')}
				{@render th('down', '↓', 'right')}
				{@render th('up', '↑', 'right')}
				{@render th('age', m.connections_col_age(), 'right')}
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
						<span class="ell" title={`${destination(c)}\n${m.connections_source({ src: source(c) })}`}>
							{destination(c)}
						</span>
						<span class="ell muted c-process" title={c.metadata.processPath || m.connections_process_unknown()}>
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
							title={m.connections_close_one_title()}
							aria-label={m.connections_close_one_title()}
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

	/* A column header is a sort button but should look like a label. */
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

	/* Narrow window: columns drop off one by one, starting with the least urgent. */
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
