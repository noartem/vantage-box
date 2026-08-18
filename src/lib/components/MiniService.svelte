<script lang="ts">
	import { api } from '$lib/api';
	import { SERVICE_LABELS, runServiceAction } from '$lib/service-actions';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { onopen }: { onopen: () => void } = $props();

	let busy = $state<string | null>(null);

	const run = $derived(app.run);
	const installed = $derived(run !== null && run.mode === 'service');
	const running = $derived(run?.running === true);
	/** The config needs TUN, but there is no service: nothing to start it with, installation is required. */
	const serviceRequired = $derived(run !== null && !installed && run.tun);
	const configMissing = $derived((app.settings?.singBox.configPath ?? '').trim() === '');

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}
</script>

<section class="section">
	<div class="head">
		<button class="title" title={m.mini_open_service_tab()} onclick={onopen}>
			<span class="section-title">{m.tabs_service()}</span>
			<Icon name="external" size={11} />
		</button>

		<span class="chip" data-tone={running ? 'good' : undefined}>
			{running ? m.service_state_running() : m.service_state_stopped()}
		</span>

		<span class="spacer"></span>
	</div>

	{#if !run}
		<p class="hint">{m.common_reading_state()}</p>
	{:else}
		<div class="form">
			<span class="lbl">{m.service_launch()}</span>
			<span>
				{installed ? m.service_system_service() : m.service_child_process()}
				{#if !installed && run.processPid !== null}
					<span class="muted mono">PID {run.processPid}</span>
				{/if}
			</span>

			{#if run.service.supported}
				<span class="lbl">{m.service_service_label()}</span>
				<span>{SERVICE_LABELS[run.service.state]()}</span>
			{/if}

			<span class="lbl">{m.service_tun_in_config()}</span>
			<span class:warnish={serviceRequired}>{run.tun ? m.service_tun_present_short() : m.service_tun_absent()}</span>

			<span class="lbl">{m.common_version()}</span>
			<span class="mono">
				{app.binaryInfo?.version ?? (app.binaryInfo?.present ? m.service_version_undefined() : m.service_no_file())}
			</span>
		</div>

		{#if configMissing}
			<div class="banner warn">{m.mini_config_missing()}</div>
		{:else if serviceRequired}
			<div class="banner warn">
				{m.service_tun_requires_service()}
			</div>
		{/if}

		<div class="toolbar">
			<button
				class="primary"
				disabled={busy !== null || running || serviceRequired || configMissing}
				onclick={() => act('start', api.start)}
			>
				{busy === 'start' ? m.service_starting() : m.service_start()}
			</button>
			<button disabled={busy !== null || !running} onclick={() => act('stop', api.stop)}>
				{busy === 'stop' ? m.service_stopping() : m.service_stop()}
			</button>
			<button
				disabled={busy !== null || !running || configMissing}
				onclick={() => act('restart', api.restart)}
			>
				{busy === 'restart' ? m.service_restarting() : m.service_restart()}
			</button>
		</div>
	{/if}
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

	.warnish {
		color: var(--fair);
	}
</style>
