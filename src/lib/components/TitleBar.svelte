<script lang="ts">
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import Icon from './Icon.svelte';
	import { app } from '$lib/state.svelte';
	import { TABS, type TabId } from '$lib/tabs';
	import { m } from '$lib/paraglide/messages.js';

	let { tab, ontab }: { tab: TabId; ontab: (next: TabId) => void } = $props();

	/** Same signal as the tray icon (tray.rs): sing-box is running and the
	 *  connection is established — the logo is "on", otherwise the default "off". */
	const active = $derived(app.run?.running === true && app.status.state === 'connected');

	/** In a regular browser (vite dev without Tauri) there is no window — the
	 *  bar simply does not control it, but the tabs keep working. */
	function win() {
		try {
			return getCurrentWindow();
		} catch {
			return null;
		}
	}

	let maximized = $state(false);

	$effect(() => {
		const w = win();
		if (!w) return;
		w.isMaximized().then((value) => (maximized = value));
		// Maximizing can happen outside our buttons too: Win+Up, Aero Snap, a
		// double-click on the drag strip.
		const unlisten = w.onResized(() => w.isMaximized().then((value) => (maximized = value)));
		return () => {
			unlisten.then((stop) => stop());
		};
	});
</script>

<!-- Preload the "on" logo so the first state change does not flash. -->
<svelte:head>
	<link rel="preload" as="image" href="/logo-on.svg" />
</svelte:head>

<!-- data-tauri-drag-region is on the strip itself and on the spacer: Tauri checks
	 exactly the event's target element, so clicks on buttons and tabs do not reach
	 here, while a double-click on empty space maximizes the window. -->
<div class="titlebar" data-tauri-drag-region>
	<span class="brand" data-tauri-drag-region>
		<!-- Logo to the left of the name. off — default, on — when the tunnel is
			 active; same signal as the tray icon. The SVG sources are placed by the
			 npm run icons script into static/logo-{off,on}.svg. -->
		<img
			class="logo"
			src={active ? '/logo-on.svg' : '/logo-off.svg'}
			alt=""
			draggable="false"
			data-tauri-drag-region
		/>
		<span class="wordmark" data-tauri-drag-region>Vantage&nbsp;Box</span>
	</span>

	<nav>
		{#each TABS as item (item.id)}
			<button
				class="tab"
				class:active={tab === item.id}
				title={item.label()}
				aria-current={tab === item.id ? 'page' : undefined}
				onclick={() => ontab(item.id)}
			>
				<Icon name={item.icon} size={14} />
				<span class="label">{item.label()}</span>
			</button>
		{/each}
	</nav>

	<div class="spacer" data-tauri-drag-region></div>

	<!-- Glyphs — from the Windows system font: custom graphics here would only
		 diverge from the native metrics of neighboring windows. -->
	<div class="controls">
		<button class="win-btn" aria-label={m.titlebar_minimize()} onclick={() => win()?.minimize()}>&#xE921;</button>
		<button
			class="win-btn"
			aria-label={maximized ? m.titlebar_restore() : m.titlebar_maximize()}
			onclick={() => win()?.toggleMaximize()}
		>
			{maximized ? '' : ''}
		</button>
		<!-- close(), not destroy(): the window must go through CloseRequested, otherwise
			 the "minimize to tray" setting stops working. -->
		<button class="win-btn close" aria-label={m.titlebar_close()} onclick={() => win()?.close()}>
			&#xE8BB;
		</button>
	</div>
</div>

<style>
	.titlebar {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-title);
		padding-left: var(--sp-4);
		background: var(--surface);
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.brand {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		padding-right: var(--sp-2);
	}

	.logo {
		width: 16px;
		height: 16px;
		flex-shrink: 0;
		/* SVG with explicit colors — does not inherit the theme, like the tray icon. */
		display: block;
	}

	.wordmark {
		font-size: var(--fs-sm);
		font-weight: 600;
		color: var(--text-muted);
		letter-spacing: 0.02em;
	}

	nav {
		display: flex;
		align-items: center;
		gap: var(--sp-1);
	}

	.tab {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: 24px;
		padding: 0 var(--sp-4);
		background: transparent;
		border-color: transparent;
		color: var(--text-muted);
	}

	.tab:hover:not(.active) {
		background: var(--surface-alt);
		border-color: transparent;
		color: var(--text);
	}

	.tab.active {
		background: var(--accent-soft);
		border-color: transparent;
		color: var(--accent);
	}

	.spacer {
		flex: 1;
		align-self: stretch;
		min-width: var(--sp-4);
	}

	.controls {
		display: flex;
		align-self: stretch;
	}

	.win-btn {
		width: var(--w-titlebar-btn);
		height: 100%;
		padding: 0;
		border: none;
		border-radius: 0;
		background: transparent;
		color: var(--text-muted);
		font-family: 'Segoe Fluent Icons', 'Segoe MDL2 Assets', sans-serif;
		font-size: 10px;
		line-height: 1;
	}

	.win-btn:hover:not(:disabled) {
		background: var(--surface-alt);
		border-color: transparent;
		color: var(--text);
	}

	.win-btn.close:hover:not(:disabled) {
		background: #c42b1c;
		color: #fff;
	}

	/* Tight — labels go into title, only icons remain: all seven tabs fit
	   even at the minimum 640px. */
	@media (max-width: 1000px) {
		.wordmark {
			display: none;
		}
	}

	@media (max-width: 860px) {
		.label {
			display: none;
		}

		.tab {
			padding: 0 var(--sp-3);
		}
	}
</style>
