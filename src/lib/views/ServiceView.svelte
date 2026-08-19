<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import InfoButton from '$lib/components/InfoButton.svelte';
	import VersionsPanel from '$lib/components/VersionsPanel.svelte';
	import { SERVICE_LABELS, runServiceAction } from '$lib/service-actions';
	import { m } from '$lib/paraglide/messages.js';
	import { runtimeConfigModal } from '$lib/runtime-config.svelte';
	import { app } from '$lib/state.svelte';
	import { tooltip } from '$lib/tooltip';
	import type { TabId } from '$lib/tabs';

	let { ongoto }: { ongoto: (tab: TabId) => void } = $props();

	let busy = $state<string | null>(null);

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
	/** runtime.json lives next to settings.json — same dir, same name pattern as
	 *  the backend's runtime_config_path() (config_dir()/runtime.json). Derived
	 *  here so a backend round-trip is not needed just to show the path. */
	const runtimePath = $derived(
		app.settingsPath ? app.settingsPath.replace(/[^\\/]+$/, 'runtime.json') : ''
	);

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}

	/** Opens a path in the system editor / folder viewer. Surface errors as an
	 *  alert instead of letting them sink silently — the file may not exist yet
	 *  (runtime.json before the first start), and the opener will then reject. */
	async function guard(action: () => Promise<unknown>) {
		try {
			await action();
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	/** Which path was just copied — flips its inline icon to a check briefly. */
	let copiedPath = $state<'config' | 'runtime' | null>(null);
	async function copyPath(which: 'config' | 'runtime') {
		const value = which === 'config' ? configPath : runtimePath;
		if (!value) return;
		await guard(async () => {
			await navigator.clipboard.writeText(value);
			copiedPath = which;
			setTimeout(() => (copiedPath = null), 1500);
		});
	}
</script>

<div class="page">
	<!-- A flex row, not CSS columns: in a row the three blocks stretch to the
		 tallest one's height, so no empty space is left between them and the
		 versions table below. Wraps to a column on narrow windows. -->
	<div class="top-row">
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
					<InfoButton label={() => m.common_explanations()}>
						<p>
							{m.service_help_restart()}
							<code class="inline">cache_file</code>.
						</p>
					</InfoButton>
				</div>

				<div class="form aligned-baseline">
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
					<!-- The path is a click-to-copy button (icon flips to a check);
					     the actions live under it. The editor is on the Config tab;
					     Open / Show in folder hand the file itself to the system. -->
					<div class="path-cell">
						<button
							class="copy-path"
							disabled={!configPath}
							title={copiedPath === 'config' ? m.common_copied() : configPath || m.service_not_set()}
							aria-label={m.common_copy()}
							onclick={() => copyPath('config')}
						>
							<code class="path ell">{configPath || m.service_not_set()}</code>
							<Icon name={copiedPath === 'config' ? 'check' : 'copy'} size={12} />
						</button>
						<div class="path-actions">
							<button class="mini" disabled={!configPath} onclick={() => ongoto('config')}>
								<Icon name="edit" size={12} />
								{m.settings_file_edit()}
							</button>
							<button
								class="mini"
								disabled={!configPath}
								onclick={() => guard(() => openPath(configPath))}
							>
								<Icon name="external" size={12} />
								{m.common_open()}
							</button>
							<button
								class="mini"
								disabled={!configPath}
								onclick={() => guard(() => revealItemInDir(configPath))}
							>
								<Icon name="folder" size={12} />
								{m.common_show_in_folder()}
							</button>
						</div>
					</div>

					<span class="lbl">{m.runtime_config_title()}</span>
					<!-- The runtime config is read-only — "Edit" opens the in-app viewer
					     modal; Open / Show in folder hand the file to the system. -->
					<div class="path-cell">
						<button
							class="copy-path"
							disabled={!runtimePath}
							title={copiedPath === 'runtime' ? m.common_copied() : runtimePath || m.service_not_set()}
							aria-label={m.common_copy()}
							onclick={() => copyPath('runtime')}
						>
							<code class="path ell">{runtimePath || m.service_not_set()}</code>
							<Icon name={copiedPath === 'runtime' ? 'check' : 'copy'} size={12} />
						</button>
						<div class="path-actions">
							<button class="mini" disabled={!runtimePath} onclick={() => runtimeConfigModal.show()}>
								<Icon name="edit" size={12} />
								{m.settings_file_edit()}
							</button>
							<button
								class="mini"
								disabled={!runtimePath}
								onclick={() => guard(() => openPath(runtimePath))}
							>
								<Icon name="external" size={12} />
								{m.common_open()}
							</button>
							<button
								class="mini"
								disabled={!runtimePath}
								onclick={() => guard(() => revealItemInDir(runtimePath))}
							>
								<Icon name="folder" size={12} />
								{m.common_show_in_folder()}
							</button>
						</div>
					</div>
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
			</section>

			{#if run.service.supported}
				<section class="section">
					<div class="head">
						<h3 class="section-title">{m.service_system_service_title()}</h3>
						<span class="chip" data-tone={installed && running ? 'good' : undefined}>
							{SERVICE_LABELS[run.service.state]()}
						</span>
						<span class="spacer"></span>
						<InfoButton label={() => m.common_explanations()}>
							<p>
								{#if installed}
									{m.service_help_installed()}
								{:else if run.tun}
									{m.service_help_tun_required()}
								{:else}
									{m.service_help_not_needed()}
								{/if}
							</p>
						</InfoButton>
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

	/* Top row of blocks: equal width, equal height (stretch), wraps when narrow.
	   Stretching closes the gap to the versions table below. */
	.top-row {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-4);
		align-items: stretch;
	}

	.top-row > :global(.section) {
		flex: 1 1 330px;
		min-width: 0;
	}

	/* The state form pairs a label with a value, then a path cell that stacks a
	   copyable path and the action buttons. Baseline, not center: the mono path
	   sits higher than the sans label, so center alignment put them on different
	   lines. Baseline puts the label and the path text on one line; the buttons
	   stay stacked under the path. */
	.aligned-baseline {
		align-items: baseline;
	}

	/* A path shown as a click-to-copy button, with the actions stacked underneath. */
	.path-cell {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: var(--sp-2);
	}

	/* Click-to-copy: the path reads like the code it replaces (mono, ellipsized,
	   accent on hover) and the icon flips to a check briefly. Resets the global
	   button chrome so it sits on the value line. */
	.copy-path {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		width: 100%;
		text-align: left;
		height: auto;
		min-height: 0;
		padding: 0;
		background: transparent;
		border: none;
		color: var(--text);
		cursor: pointer;
	}

	.copy-path:hover:not(:disabled) {
		border: none;
		color: var(--accent);
	}

	.copy-path .path {
		flex: 1;
		min-width: 0;
	}

	/* The row of follow-up actions under a path: edit / open / show in folder. */
	.path-actions {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}

	/* A compact button under a path — smaller than the 22px service controls so
	   it reads as a follow-up action, not a primary one. */
	.mini {
		height: auto;
		min-height: 0;
		padding: var(--sp-1) var(--sp-3);
		font-size: var(--fs-sm);
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
