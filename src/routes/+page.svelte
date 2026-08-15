<script lang="ts">
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import StatusBar from '$lib/components/StatusBar.svelte';
	import { app } from '$lib/state.svelte';
	import ConfigView from '$lib/views/ConfigView.svelte';
	import ConnectionsView from '$lib/views/ConnectionsView.svelte';
	import Dashboard from '$lib/views/Dashboard.svelte';
	import LogsView from '$lib/views/LogsView.svelte';
	import Onboarding from '$lib/views/Onboarding.svelte';
	import ProxyPopup from '$lib/views/ProxyPopup.svelte';
	import ServiceView from '$lib/views/ServiceView.svelte';
	import SettingsView from '$lib/views/SettingsView.svelte';
	import SubscriptionsView from '$lib/views/SubscriptionsView.svelte';

	// Попап открывается тем же бандлом во втором окне. Различаем окна по метке,
	// а не по адресу: любой путь или query увёл бы SvelteKit на несуществующий
	// маршрут и вместо попапа показал бы 404.
	const isPopup = currentWindowLabel() === 'popup';

	function currentWindowLabel(): string {
		try {
			return getCurrentWindow().label;
		} catch {
			// Открыли в обычном браузере — значит, это главное окно.
			return 'main';
		}
	}

	// Окно приложения — не браузер: вкладки держим в состоянии, без роутинга и URL.
	const TABS = [
		{ id: 'dashboard', label: 'Дашборд' },
		{ id: 'connections', label: 'Соединения' },
		{ id: 'subscriptions', label: 'Подписки' },
		{ id: 'config', label: 'Конфиг' },
		{ id: 'logs', label: 'Логи' },
		{ id: 'service', label: 'Сервис' },
		{ id: 'settings', label: 'Настройки' }
	] as const;

	let tab = $state<(typeof TABS)[number]['id']>('dashboard');

	onMount(() => {
		// Попапу общее состояние не нужно: он сам ходит за списком групп.
		if (!isPopup) app.start();
	});
</script>

{#if isPopup}
	<ProxyPopup />
{:else}
<div class="shell">
	<nav>
		<div class="brand">Vantage Box</div>
		{#each TABS as item (item.id)}
			<button class="tab" class:active={tab === item.id} onclick={() => (tab = item.id)}>
				{item.label}
			</button>
		{/each}
	</nav>

	<div class="content">
		<StatusBar />

		<main>
			{#if tab === 'dashboard'}
				<Dashboard />
			{:else if tab === 'connections'}
				<ConnectionsView />
			{:else if tab === 'subscriptions'}
				<SubscriptionsView />
			{:else if tab === 'config'}
				<ConfigView />
			{:else if tab === 'logs'}
				<LogsView />
			{:else if tab === 'service'}
				<ServiceView />
			{:else}
				<SettingsView />
			{/if}
		</main>
	</div>
</div>

<!-- Онбординг первого запуска: поверх всего, пока не выбраны бинарник и конфиг. -->
{#if app.needsOnboarding}<Onboarding />{/if}
{/if}

<style>
	.shell {
		display: grid;
		grid-template-columns: 180px 1fr;
		height: 100vh;
	}

	nav {
		background: var(--surface);
		border-right: 1px solid var(--border);
		padding: 16px 12px;
		display: grid;
		gap: 4px;
		align-content: start;
	}

	.brand {
		font-weight: 600;
		padding: 0 8px 12px;
	}

	.tab {
		background: transparent;
		border-color: transparent;
		text-align: left;
	}

	.tab.active {
		background: var(--accent-soft);
		border-color: transparent;
		color: var(--accent);
	}

	.content {
		display: grid;
		grid-template-rows: auto 1fr;
		gap: 12px;
		padding: 16px 20px;
		min-height: 0;
	}

	main {
		overflow-y: auto;
		min-height: 0;
	}
</style>
