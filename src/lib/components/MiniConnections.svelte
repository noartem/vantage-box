<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { destination, outbound, processName, source } from '$lib/connection';
	import { formatBytes } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { onopen }: { onopen: () => void } = $props();

	/** Столько же строк, сколько в мини-логах: панели стоят в одном ряду. */
	const ROWS = 12;

	let busy = $state<string | null>(null);

	/** Самые «тяжёлые» соединения: при сотне открытых сокетов интересны те,
	 *  через которые реально идёт трафик, а не самые свежие. */
	const top = $derived([...app.connections].sort((a, b) => b.download - a.download).slice(0, ROWS));

	async function guard(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await call();
			// Следующий кадр /connections приедет сам — обновлять вручную не нужно.
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
		}
	}
</script>

<section class="section">
	<div class="head">
		<button class="title" title="Открыть вкладку «Соединения»" onclick={onopen}>
			<span class="section-title">Соединения</span>
			<Icon name="external" size={11} />
		</button>

		<span class="muted mono counter">{app.connections.length}</span>

		<span class="spacer"></span>

		<span class="muted mono totals">
			↓ {formatBytes(app.connectionTotals.down)} · ↑ {formatBytes(app.connectionTotals.up)}
		</span>

		<button
			class="icon-btn"
			title="Закрыть все соединения"
			aria-label="Закрыть все"
			disabled={busy !== null || app.connections.length === 0}
			onclick={() => guard('all', api.closeAllConnections)}
		>
			<Icon name="trash" size={13} />
		</button>
	</div>

	<!-- Высота фиксируется только когда есть что показывать: пустой список не
		 должен держать двенадцать строк белого места. -->
	<div class="list" class:filled={top.length > 0}>
		{#if app.status.state !== 'connected'}
			<p class="hint">Нет связи с Clash API — sing-box не запущен.</p>
		{:else if top.length === 0}
			<p class="hint">Активных соединений нет.</p>
		{:else}
			{#each top as c (c.id)}
				<div class="row">
					<span class="ell" title="{destination(c)}&#10;источник {source(c)}">{destination(c)}</span>
					<span class="ell muted" title={c.metadata.processPath || 'процесс неизвестен'}>
						{processName(c)}
					</span>
					<span class="ell mono" title={c.chains.join(' ← ')}>{outbound(c)}</span>
					<span class="mono right">{formatBytes(c.download)}</span>
					<button
						class="icon-btn"
						title="Закрыть соединение"
						aria-label="Закрыть соединение"
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

	/* Заголовок — кнопка перехода, но выглядеть должен подписью секции. */
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

	/* Высота по числу строк: список то пустеет, то заполняется, и без этого
	   соседние панели ряда прыгали бы вслед за ним. */
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
