<script lang="ts">
	import { api } from '$lib/api';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import VersionsPanel from '$lib/components/VersionsPanel.svelte';
	import { SERVICE_LABELS, runServiceAction } from '$lib/service-actions';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import { tooltip } from '$lib/tooltip';

	let busy = $state<string | null>(null);
	let help = $state(false);

	/** Tooltip text over buttons blocked by a running service. Lazy function:
	 *  m.x() reads the locale at call time, not at module load. */
	const SERVICE_RUNNING_HINT = () => m.service_running_hint();

	const run = $derived(app.run);
	const installed = $derived(run !== null && run.mode === 'service');
	const running = $derived(run?.running === true);
	/** The service is actually running — reinstall/uninstall at this point would
	 *  tear down the VPN, so the buttons are disabled. */
	const serviceRunning = $derived(run?.service.state === 'running');
	/** The config needs TUN, but there is no service: nothing to start it with, installation is required. */
	const serviceRequired = $derived(run !== null && !installed && run.tun);
	const configPath = $derived(app.settings?.singBox.configPath ?? '');
	const configMissing = $derived(configPath.trim() === '');

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}
</script>

<div class="page">
	<!-- Cards flow through columns: in a grid a row was as tall as the tallest
		 section, and under a short one there was empty space to the end of the row. -->
	<div class="masonry">
		{#if !run}
			<p class="hint">{m.common_reading_state()}</p>
		{:else}
			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.service_section_state()}</h3>
					<span class="chip" data-tone={running ? 'good' : undefined}>
						{running ? m.service_state_running() : m.service_state_stopped()}
					</span>
					<span class="spacer"></span>
					<button
						class="icon-btn"
						class:on={help}
						title={m.common_explanations()}
						aria-label={m.common_explanations()}
						onclick={() => (help = !help)}
					>
						<Icon name="info" size={13} />
					</button>
				</div>

				<div class="form">
					<span class="lbl">{m.service_launch()}</span>
					<span>
						{installed ? m.service_system_service() : m.service_child_process()}
						{#if !installed && run.processPid !== null}
							<span class="muted mono">PID {run.processPid}</span>
						{/if}
					</span>

					<span class="lbl">{m.service_tun_in_config()}</span>
					<span class:warnish={serviceRequired}>
						{run.tun ? m.service_tun_present() : m.service_tun_absent()}
					</span>

					<span class="lbl">{m.common_config()}</span>
					<code class="path ell selectable" title={configPath || m.service_not_set()}>
						{configPath || m.service_not_set()}
					</code>
				</div>

				{#if configMissing}
					<div class="banner warn">
						{m.service_config_missing_hint()}
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
						{busy === 'restart' ? m.service_restarting() : m.service_soft_restart()}
					</button>
				</div>

				{#if help}
					<p class="hint">
						{m.service_help_restart()}
						<code class="inline">cache_file</code>.
					</p>
				{/if}
			</section>

			{#if run.service.supported}
				<section class="section">
					<div class="head">
						<h3 class="section-title">{m.service_system_service_title()}</h3>
						<span class="chip" data-tone={installed && running ? 'good' : undefined}>
							{SERVICE_LABELS[run.service.state]()}
						</span>
						<span class="spacer"></span>
					</div>

					<div class="form">
						<span class="lbl">{m.common_name()}</span>
						<code class="path ell selectable" title={run.service.name}>{run.service.name}</code>
					</div>

					{#if run.service.detail}
						<div class="banner warn">{run.service.detail}</div>
					{/if}

					{#if serviceRequired}
						<div class="banner warn">
							{m.service_tun_requires_service()}
						</div>
					{/if}

					<div class="toolbar">
						<span class="tip" use:tooltip={serviceRunning ? SERVICE_RUNNING_HINT() : ''}>
							<button
								class:primary={run.tun && !installed}
								disabled={busy !== null || serviceRunning || configMissing}
								onclick={() => act('install', api.installService)}
							>
								{#if busy === 'install'}
									{installed ? m.service_reinstalling() : m.service_installing()}
								{:else}
									{installed ? m.service_reinstall() : m.service_install_service()}
								{/if}
							</button>
						</span>
						{#if installed}
							<span class="tip" use:tooltip={serviceRunning ? SERVICE_RUNNING_HINT() : ''}>
								<button
									class="danger"
									disabled={busy !== null || serviceRunning}
									onclick={() => act('uninstall', api.uninstallService)}
								>
									{busy === 'uninstall' ? m.service_uninstalling() : m.service_uninstall()}
								</button>
							</span>
						{/if}
					</div>

					{#if help}
						<p class="hint">
							{#if installed}
								{m.service_help_installed()}
							{:else if run.tun}
								{m.service_help_tun_required()}
							{:else}
								{m.service_help_not_needed()}
							{/if}
						</p>
					{/if}
				</section>
			{:else}
				<p class="hint">{run.service.detail}</p>
			{/if}
		{/if}

		<BinaryPanel />
	</div>

	<!-- The version catalog is a table: in a 330px column it would be unreadable,
		 so it lives below the tile spanning the full width. -->
	<VersionsPanel />
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
	}

	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
		max-width: 100%;
	}

	.warnish {
		color: var(--fair);
	}

	.hint {
		max-width: 62ch;
	}
</style>
