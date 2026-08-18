<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { destination, outbound, processName, source } from '$lib/connection';
	import { formatBytes } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { onopen, active = true }: { onopen: () => void; active?: boolean } = $props();

	/** Same number of rows as in mini-logs: the panels stand in one row. */
	const ROWS = 12;

	let busy = $state<string | null>(null);

	/** The "heaviest" connections: with a hundred open sockets, the ones that
	 *  actually carry traffic are interesting, not the freshest. Sorting all
	 *  connections every frame is the main cost of the panel, so while the tab is
	 *  inactive we do not pay it; the block's height is held by .filled. */
	const top = $derived.by(() => {
		if (!active) return [];
		return [...app.connections].sort((a, b) => b.download - a.download).slice(0, ROWS);
	});

	async function guard(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await call();
			// The next /connections frame arrives on its own — no manual refresh needed.
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
		}
	}
</script>

<section class="section">
	<div class="head">
		<button class="title" title={m.mini_open_connections_tab()} onclick={onopen}>
			<span class="section-title">{m.tabs_connections()}</span>
			<Icon name="external" size={11} />
		</button>

		<span class="muted mono counter">{app.connections.length}</span>

		<span class="spacer"></span>

		<span class="muted mono totals">
			↓ {formatBytes(app.connectionTotals.down)} · ↑ {formatBytes(app.connectionTotals.up)}
		</span>

		<button
			class="icon-btn"
			title={m.connections_close_all_title()}
			aria-label={m.connections_close_all_short()}
			disabled={busy !== null || app.connections.length === 0}
			onclick={() => guard('all', api.closeAllConnections)}
		>
			<Icon name="trash" size={13} />
		</button>
	</div>

	<!-- Height is fixed only when there is something to show: an empty list
		 should not hold twelve rows of whitespace. filled depends on the presence
		 of data, not on active — a hidden block keeps the same height as a visible one. -->
	<div class="list" class:filled={app.connections.length > 0}>
		{#if !active}
			<!-- tab inactive: do not render rows, height is held by .filled -->
		{:else if app.status.state !== 'connected'}
			<p class="hint">{m.connections_no_api()}</p>
		{:else if top.length === 0}
			<p class="hint">{m.connections_none()}</p>
		{:else}
			{#each top as c (c.id)}
				<div class="row">
					<span class="ell" title={`${destination(c)}\n${m.connections_source({ src: source(c) })}`}>{destination(c)}</span>
					<span class="ell muted" title={c.metadata.processPath || m.connections_process_unknown()}>
						{processName(c)}
					</span>
					<span class="ell mono" title={c.chains.join(' ← ')}>{outbound(c)}</span>
					<span class="mono right">{formatBytes(c.download)}</span>
					<button
						class="icon-btn"
						title={m.connections_close_one_title()}
						aria-label={m.connections_close_one_title()}
						disabled={busy !== null}
						onclick={() => guard(c.id, () => api.closeConnection(c.id))}
					>
						<Icon name="close" size={11} />
					</button>
				</div>
			{/each}
		{/if}
	</div>
</section>

<style>
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	/* The title is a navigation button but should look like a section label. */
	.title {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		height: auto;
		padding: 0;
		background: transparent;
		border: none;
		color: var(--text-muted);
	}

	.title:hover:not(:disabled) {
		border: none;
		color: var(--accent);
	}

	.counter,
	.totals {
		font-size: var(--fs-sm);
		white-space: nowrap;
	}

	.list {
		overflow: hidden;
		font-size: var(--fs-sm);
	}

	/* Height by row count: the list empties and fills, and without this the
	   neighboring panels in the row would jump along with it. */
	.list.filled {
		height: calc(12 * var(--h-row));
	}

	.row {
		display: grid;
		grid-template-columns: minmax(80px, 2fr) minmax(60px, 1fr) minmax(60px, 1fr) 62px var(--h-ctl);
		gap: var(--sp-3);
		align-items: center;
		height: var(--h-row);
	}

	.row:hover {
		background: var(--surface-alt);
	}

	.right {
		text-align: right;
	}
</style>
