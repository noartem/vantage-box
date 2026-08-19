<script lang="ts">
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import AlertStrip from '$lib/components/AlertStrip.svelte';
	import RuntimeConfigModal from '$lib/components/RuntimeConfigModal.svelte';
	import StatusBar from '$lib/components/StatusBar.svelte';
	import TitleBar from '$lib/components/TitleBar.svelte';
	import { app } from '$lib/state.svelte';
	import { TABS, loadTab, saveTab, type TabId } from '$lib/tabs';
	import ConfigView from '$lib/views/ConfigView.svelte';
	import ConnectionsView from '$lib/views/ConnectionsView.svelte';
	import Dashboard from '$lib/views/Dashboard.svelte';
	import LogsView from '$lib/views/LogsView.svelte';
	import Onboarding from '$lib/views/Onboarding.svelte';
	import ProxyPopup from '$lib/views/ProxyPopup.svelte';
	import ServiceView from '$lib/views/ServiceView.svelte';
	import SettingsView from '$lib/views/SettingsView.svelte';
	import SubscriptionsView from '$lib/views/SubscriptionsView.svelte';

	// The popup is opened by the same bundle in a second window. We tell windows
	// apart by label, not by URL: any path or query would send SvelteKit to a
	// nonexistent route and show a 404 instead of the popup.
	const isPopup = currentWindowLabel() === 'popup';

	function currentWindowLabel(): string {
		try {
			return getCurrentWindow().label;
		} catch {
			// Opened in a regular browser — so this is the main window.
			return 'main';
		}
	}

	/** The tab survives a restart: the app is opened for what we stopped on,
	 *  not for the dashboard. */
	const initialTab = loadTab();
	let tab = $state<TabId>(initialTab);
	/** Which tabs have already been opened. A component mounts on first open and
	 *  stays in the DOM until the window closes — so re-entering does not flash
	 *  an empty loader and does not reset state (filters, drafts, scroll). */
	let opened = $state<Record<string, boolean>>({ [initialTab]: true });

	function goto(next: TabId) {
		tab = next;
		saveTab(next);
		opened[next] = true;
	}

	/** Ctrl+1…7 — tabs in order, like in a browser. Ctrl+Tab / Ctrl+Shift+Tab —
	 *  cycle through tabs forward and back. Ctrl+Alt+S — settings.
	 *  Hotkey recording in settings intercepts the press before us and cancels it —
	 *  we respect that. */
	function onKeydown(event: KeyboardEvent) {
		if (isPopup || event.defaultPrevented) return;
		if (!event.ctrlKey) return;

		// Ctrl+Alt+S — settings. Check before the general Alt filter, otherwise
		// this hotkey never takes effect.
		if (event.altKey && !event.shiftKey && event.key.toLowerCase() === 's') {
			event.preventDefault();
			goto('settings');
			return;
		}

		if (event.altKey) return;

		if (event.key === 'Tab') {
			event.preventDefault();
			const current = TABS.findIndex((t) => t.id === tab);
			const dir = event.shiftKey ? -1 : 1;
			const next = (current + dir + TABS.length) % TABS.length;
			goto(TABS[next].id);
			return;
		}

		// Ctrl+Shift+W — close the window. close() goes through CloseRequested, so
		// the "minimize to tray" setting keeps working.
		if (event.shiftKey && event.code === 'KeyW') {
			event.preventDefault();
			getCurrentWindow().close();
			return;
		}

		if (event.shiftKey) return;
		const index = Number(event.key) - 1;
		if (!Number.isInteger(index) || index < 0 || index >= TABS.length) return;
		event.preventDefault();
		goto(TABS[index].id);
	}

	onMount(() => {
		// The popup does not need the shared state: it fetches the group list itself.
		if (!isPopup) app.start();
	});
</script>

<svelte:window onkeydown={onKeydown} />

{#if isPopup}
	<ProxyPopup />
{:else}
	<!-- Frameless window: title, tabs and window controls are one 32px strip.
		 The alert row between it and the content either takes exactly 24px or does
		 not exist, so the content never shifts. -->
	<div class="shell">
		<TitleBar {tab} ontab={goto} />

		<!-- The wrapper is required: with no alerts AlertStrip renders no nodes,
			 and then the grid rows shift — the status bar lands in the stretching
			 row instead of the bottom one. An empty div honestly takes its 0px. -->
		<div class="alerts">
			<AlertStrip ongoto={goto} />
		</div>

		<main>
			{#each TABS as t (t.id)}
				{#if opened[t.id]}
					<!-- Not {#if tab === ...}: otherwise switching destroys the tab and
						 rebuilds it — hence the empty flash and the lost state.
						 We hide via a CSS class; the nodes stay alive. -->
					<div class="panel" class:hidden={tab !== t.id} aria-hidden={tab !== t.id}>
						{#if t.id === 'dashboard'}
							<Dashboard ongoto={goto} active={tab === 'dashboard'} />
						{:else if t.id === 'connections'}
							<ConnectionsView active={tab === 'connections'} />
						{:else if t.id === 'subscriptions'}
							<SubscriptionsView />
						{:else if t.id === 'config'}
							<ConfigView />
						{:else if t.id === 'logs'}
							<LogsView active={tab === 'logs'} />
						{:else if t.id === 'service'}
							<ServiceView ongoto={goto} />
						{:else if t.id === 'settings'}
							<SettingsView active={tab === 'settings'} />
						{/if}
					</div>
				{/if}
			{/each}
		</main>

		<StatusBar />
	</div>

	<!-- Read-only viewer for the running config (runtime.json). Hosted at the top
	     level so the error alert can open it regardless of the active tab. -->
	<RuntimeConfigModal />

	<!-- First-run onboarding: on top of everything until a binary and config are chosen. -->
	{#if app.needsOnboarding}<Onboarding />{/if}
{/if}

<style>
	.shell {
		display: grid;
		grid-template-rows: var(--h-title) auto 1fr var(--h-status);
		height: 100vh;
	}

	.alerts {
		min-width: 0;
	}

	main {
		overflow-y: auto;
		min-height: 0;
		padding: var(--sp-4);
	}

	/* Each tab panel fills main; hidden ones are taken out of layout but the
	   nodes stay alive. height: 100% is needed by views with their own scroll
	   (connections, logs). */
	.panel {
		height: 100%;
		min-height: 0;
	}

	.panel.hidden {
		display: none;
	}
</style>
