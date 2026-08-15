<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { formatBytes } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import type { Connection } from '$lib/types';

	let error = $state<string | null>(null);
	let busy = $state<string | null>(null);
	let filter = $state('');

	/** Какой outbound несёт соединение — последний элемент цепочки. */
	function outbound(c: Connection): string {
		return c.chains.length > 0 ? c.chains[c.chains.length - 1] : '—';
	}

	/** Цель соединения: хост, если есть, иначе ip:port. */
	function destination(c: Connection): string {
		const m = c.metadata;
		if (m.host) return m.host;
		if (m.destinationIP) return `${m.destinationIP}:${m.destinationPort}`;
		return '—';
	}

	function source(c: Connection): string {
		return `${c.metadata.sourceIP}:${c.metadata.sourcePort}`;
	}

	const filtered = $derived(
		filter.trim() === ''
			? app.connections
			: app.connections.filter((c) => {
					const q = filter.trim().toLowerCase();
					return (
						c.metadata.host.toLowerCase().includes(q) ||
						outbound(c).toLowerCase().includes(q) ||
						c.rule.toLowerCase().includes(q) ||
						source(c).includes(q) ||
						destination(c).toLowerCase().includes(q)
					);
				})
	);

	async function closeOne(id: string) {
		busy = id;
		error = null;
		try {
			await api.closeConnection(id);
			// Следующий кадр /connections сам приедет — обновлять вручную не нужно.
		} catch (e) {
			error = errorText(e);
		} finally {
			busy = null;
		}
	}

	async function closeAll() {
		busy = 'all';
		error = null;
		try {
			await api.closeAllConnections();
		} catch (e) {
			error = errorText(e);
		} finally {
			busy = null;
		}
	}

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

<div class="page">
	<section class="card toolbar">
		<div class="info">
			<h3>Соединения</h3>
			<span class="muted">{app.connections.length} активных</span>
			<span class="muted totals">
				↓ {formatBytes(app.connectionTotals.down)} · ↑ {formatBytes(app.connectionTotals.up)}
			</span>
		</div>
		<div class="actions">
			<input bind:value={filter} placeholder="фильтр: хост, outbound, правило…" />
			<button
				disabled={busy !== null || app.connections.length === 0}
				onclick={closeAll}
			>
				{busy === 'all' ? 'Закрываю…' : 'Закрыть все'}
			</button>
		</div>
	</section>

	{#if error}
		<div class="banner">{error}</div>
	{/if}

	{#if app.status.state !== 'connected'}
		<p class="muted">Нет связи с Clash API — sing-box не запущен.</p>
	{:else if app.connections.length === 0}
		<p class="muted">Активных соединений нет.</p>
	{:else if filtered.length === 0}
		<p class="muted">Ничего не подходит под фильтр.</p>
	{:else}
		<section class="card">
			<div class="table">
				<div class="head row">
					<span>Хост</span>
					<span>Сеть</span>
					<span>Outbound</span>
					<span>Правило</span>
					<span class="num">↓</span>
					<span class="num">↑</span>
					<span></span>
				</div>
				{#each filtered as c (c.id)}
					<div class="row">
						<span class="host" title={destination(c)}>
							<span class="dest">{destination(c)}</span>
							<span class="muted src">{source(c)}</span>
						</span>
						<span class="muted">{c.metadata.network}/{c.metadata.type || 'tcp'}</span>
						<span class="outbound">{outbound(c)}</span>
						<span class="muted rule">{c.rule}{c.rulePayload ? `(${c.rulePayload})` : ''}</span>
						<span class="num">{formatBytes(c.download)}</span>
						<span class="num">{formatBytes(c.upload)}</span>
						<span class="row-actions">
							<button
								disabled={busy !== null}
								onclick={() => closeOne(c.id)}
							>
								{busy === c.id ? '…' : 'Закрыть'}
							</button>
						</span>
					</div>
				{/each}
			</div>
		</section>
	{/if}
</div>

<style>
	.page {
		display: grid;
		gap: 12px;
		align-content: start;
	}

	.toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		flex-wrap: wrap;
	}

	.info {
		display: flex;
		align-items: baseline;
		gap: 12px;
	}

	.totals {
		font-family: var(--mono);
		font-size: 12px;
	}

	.actions {
		display: flex;
		gap: 8px;
	}

	.table {
		display: grid;
		font-size: 12px;
		overflow-x: auto;
	}

	.row {
		display: grid;
		grid-template-columns: 1.6fr 0.7fr 1fr 1.2fr 0.7fr 0.7fr auto;
		gap: 10px;
		align-items: center;
		padding: 5px 6px;
		border-bottom: 1px solid var(--border);
		min-width: 720px;
	}

	.head {
		color: var(--text-muted);
		font-weight: 600;
		position: sticky;
		top: 0;
		background: var(--surface);
	}

	.host {
		display: grid;
		gap: 1px;
		min-width: 0;
	}

	.dest {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.src {
		font-size: 11px;
	}

	.num {
		font-family: var(--mono);
		text-align: right;
		font-variant-numeric: tabular-nums;
	}

	.outbound {
		font-family: var(--mono);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.rule {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.row-actions {
		display: flex;
		justify-content: flex-end;
	}
</style>