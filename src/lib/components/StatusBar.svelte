<script lang="ts">
	import { api } from '$lib/api';
	import { runServiceAction } from '$lib/service-actions';
	import { app } from '$lib/state.svelte';
	import { formatBytes, formatSpeed } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import Icon from './Icon.svelte';

	let busy = $state<string | null>(null);

	const label = $derived(
		{
			connected: m.status_connected(),
			connecting: m.status_connecting(),
			disconnected: m.status_disconnected()
		}[app.status.state]
	);

	const run = $derived(app.run);
	const running = $derived(run?.running === true);
	/** The config needs TUN, but the service is not installed — nothing to start it with. */
	const blocked = $derived(run !== null && run.mode === 'process' && run.tun);
	const total = $derived(app.totals.up + app.totals.down);

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}
</script>

<footer>
	<span class="dot" data-state={app.status.state}></span>
	<span class="state meta" title={label}>{label}</span>

	{#if app.status.version}
		<span class="meta muted selectable" title={`sing-box ${app.status.version}`}>
			sing-box {app.status.version}
		</span>
	{/if}

	{#if run}
		<span class="meta muted" title={run.mode === 'service' ? m.status_service_launch() : m.status_process_launch()}>
			{run.mode === 'service' ? m.status_service_short() : m.status_process_short()}
		</span>
	{/if}

	<span class="spacer"></span>

	{#if app.memory.inuse > 0}
		<span class="stat muted mono" title={m.status_memory_title()}>{m.status_memory_label()} {formatBytes(app.memory.inuse)}</span>
	{/if}

	<!-- Width is fixed: the digits change once a second, and without it the
		 neighboring elements would jump back and forth. -->
	<span class="rate mono" title={m.status_download_speed()}><span class="arrow">↓</span>{formatSpeed(app.traffic.down)}</span>
	<span class="rate mono" title={m.status_upload_speed()}><span class="arrow">↑</span>{formatSpeed(app.traffic.up)}</span>
	<span class="stat muted mono" title={m.status_session_total()}>∑ {formatBytes(total)}</span>

	<div class="actions">
		<button
			class="icon-btn"
			title={blocked ? m.status_start_blocked() : m.status_start_title()}
			aria-label={m.status_start_label()}
			disabled={busy !== null || running || blocked}
			onclick={() => act('start', api.start)}
		>
			<Icon name="play" size={12} fill />
		</button>
		<button
			class="icon-btn"
			title={m.status_stop_title()}
			aria-label={m.status_stop_label()}
			disabled={busy !== null || !running}
			onclick={() => act('stop', api.stop)}
		>
			<Icon name="stop" size={12} fill />
		</button>
		<button
			class="icon-btn"
			title={m.status_restart_title()}
			aria-label={m.status_restart_label()}
			disabled={busy !== null || !running}
			onclick={() => act('restart', api.restart)}
		>
			<Icon name="restart" size={13} />
		</button>
	</div>
</footer>

<style>
	footer {
		display: flex;
		align-items: center;
		gap: var(--sp-4);
		height: var(--h-status);
		padding: 0 var(--sp-3) 0 var(--sp-4);
		font-size: var(--fs-sm);
		background: var(--surface);
		border-top: 1px solid var(--border);
		flex-shrink: 0;
		/* One row — text wrapping is forbidden everywhere. */
		white-space: nowrap;
		overflow: hidden;
		min-width: 0;
	}

	.state {
		font-weight: 600;
	}

	/* Left part: status and labels. When space is short they compress and are
	   truncated with an ellipsis; the full text is shown by the system tooltip
	   (title). The right part (telemetry, buttons) has higher priority — it does
	   not compress. */
	.meta {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.dot,
	.stat,
	.rate,
	.actions {
		flex-shrink: 0;
	}

	.rate {
		min-width: 78px;
		text-align: right;
	}

	.arrow {
		color: var(--text-muted);
		margin-right: 2px;
	}

	.actions {
		display: flex;
		gap: var(--sp-1);
		margin-left: var(--sp-2);
	}
</style>
