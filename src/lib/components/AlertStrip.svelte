<script lang="ts">
	import Icon from './Icon.svelte';
	import { dismissAlert, transientAlerts, type AlertSeverity } from '$lib/alerts.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { TabId } from '$lib/tabs';

	let { ongoto }: { ongoto: (tab: TabId) => void } = $props();

	type Item = {
		key: string;
		severity: AlertSeverity;
		text: string;
		action?: { label: string; disabled?: boolean; run: () => void };
		/** When the action slot is taken by a custom action, dismiss moves to a
		 *  separate ✕ button so the alert stays dismissible. */
		dismissable?: boolean;
		dismissId?: number;
	};

	/** Previously each problem was its own banner in the header, and the top row
	 *  of the window jumped from 33px to 250px. Now they are all one list, of which
	 *  exactly one entry is visible: the strip's height stays constant, the content does not shift. */
	const items = $derived.by<Item[]>(() => {
		const list: Item[] = [];

		if (app.settingsProblem) {
			list.push({
				key: 'settings',
				severity: 'error',
				text: m.alert_settings_problem({ detail: app.settingsProblem }),
				action: { label: m.tabs_settings(), run: () => ongoto('settings') }
			});
		}

		if (app.status.state === 'disconnected' && app.status.error && app.run?.running) {
			list.push({
				key: 'api',
				severity: 'error',
				text: m.alert_api_down({ detail: app.status.error }),
				action: { label: m.tabs_config(), run: () => ongoto('config') }
			});
		}

		if (app.run?.configProblem) {
			list.push({
				key: 'config',
				severity: 'error',
				text: m.alert_config_read_failed({ detail: app.run.configProblem }),
				action: { label: m.tabs_config(), run: () => ongoto('config') }
			});
		}

		for (const alert of transientAlerts.items) {
			if (alert.action) {
				// A custom action (e.g. "Running config") takes the action slot;
				// dismiss still works via the separate ✕ button.
				list.push({
					key: `t${alert.id}`,
					severity: alert.severity,
					text: alert.text,
					action: { label: alert.action.label, run: alert.action.run },
					dismissable: true,
					dismissId: alert.id
				});
			} else {
				list.push({
					key: `t${alert.id}`,
					severity: alert.severity,
					text: alert.text,
					action: { label: m.common_hide(), run: () => dismissAlert(alert.id) }
				});
			}
		}

		// The config needs TUN, but the service is not installed — nothing to start it with.
		if (app.run?.mode === 'process' && app.run.tun) {
			list.push({
				key: 'tun',
				severity: 'warn',
				text: m.alert_tun_admin(),
				action: { label: m.tabs_service(), run: () => ongoto('service') }
			});
		}

		if (app.status.compatibility === 'tooNew' || app.status.compatibility === 'tooOld') {
			list.push({
				key: 'compat',
				severity: 'warn',
				text: m.alert_compat_out_of_range({ version: app.status.version ?? '' })
			});
		}

		if (app.updateAvailable) {
			list.push({
				key: 'update',
				severity: 'ok',
				text: m.alert_update_available({ version: app.updateAvailable.version }),
				action: {
					label: app.updateInstalling ? m.alert_installing() : m.alert_update_now(),
					disabled: app.updateInstalling,
					run: () => app.installAppUpdate()
				}
			});
		}

		return list;
	});

	let cursor = $state(0);
	const position = $derived(items.length === 0 ? 0 : Math.min(cursor, items.length - 1));
	const current = $derived(items[position]);

	function step(delta: number) {
		cursor = (position + delta + items.length) % items.length;
	}
</script>

{#if current}
	<div class="strip" data-severity={current.severity}>
		<Icon name={current.severity === 'ok' ? 'info' : 'alert'} size={13} />

		<span class="text ell selectable" title={current.text}>{current.text}</span>

		{#if current.action}
			<button class="act" disabled={current.action.disabled} onclick={current.action.run}>
				{current.action.label}
			</button>
		{/if}

		{#if current.dismissable}
			<button
				class="icon-btn"
				aria-label={m.common_hide()}
				title={m.common_hide()}
				onclick={() => dismissAlert(current.dismissId!)}
			>
				<Icon name="close" size={12} />
			</button>
		{/if}

		{#if items.length > 1}
			<button class="icon-btn" aria-label={m.alert_prev()} onclick={() => step(-1)}>
				<Icon name="chevronLeft" size={12} />
			</button>
			<span class="count mono">{position + 1}/{items.length}</span>
			<button class="icon-btn" aria-label={m.alert_next()} onclick={() => step(1)}>
				<Icon name="chevronRight" size={12} />
			</button>
		{/if}
	</div>
{/if}

<style>
	.strip {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-alert);
		padding: 0 var(--sp-4);
		font-size: var(--fs-sm);
		border-bottom: 1px solid var(--border);
		color: var(--poor);
		background: color-mix(in srgb, var(--poor) 12%, var(--bg));
	}

	.strip[data-severity='warn'] {
		color: var(--fair);
		background: color-mix(in srgb, var(--fair) 12%, var(--bg));
	}

	.strip[data-severity='ok'] {
		color: var(--good);
		background: color-mix(in srgb, var(--good) 12%, var(--bg));
	}

	/* The text takes all free space and is truncated; the full text is in title —
	   so the row can never grow a second line. */
	.text {
		flex: 1;
	}

	/* The button inside the strip is shorter than usual: 22px does not fit a 24px row. */
	.act {
		height: 18px;
		padding: 0 var(--sp-3);
		font-size: var(--fs-xs);
		background: transparent;
		border-color: currentcolor;
		color: inherit;
	}

	.act:hover:not(:disabled) {
		background: color-mix(in srgb, currentcolor 15%, transparent);
		border-color: currentcolor;
	}

	.strip .icon-btn {
		width: 18px;
		height: 18px;
		color: inherit;
	}

	.count {
		font-size: var(--fs-xs);
		opacity: 0.8;
	}
</style>
