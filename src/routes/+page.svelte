<script lang="ts">
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import AlertStrip from '$lib/components/AlertStrip.svelte';
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

	let tab = $state<TabId>(loadTab());

	function goto(next: TabId) {
		tab = next;
		saveTab(next);
	}

	/** Ctrl+1…7 — вкладки по порядку, как в браузере. Запись хоткея в настройках
	 *  перехватывает нажатие раньше нас и гасит его — уважаем это. */
	function onKeydown(event: KeyboardEvent) {
		if (isPopup || event.defaultPrevented) return;
		if (!event.ctrlKey || event.altKey || event.shiftKey) return;
		const index = Number(event.key) - 1;
		if (!Number.isInteger(index) || index < 0 || index >= TABS.length) return;
		event.preventDefault();
		goto(TABS[index].id);
	}

	onMount(() => {
		// Попапу общее состояние не нужно: он сам ходит за списком групп.
		if (!isPopup) app.start();
	});
</script>

<svelte:window onkeydown={onKeydown} />

{#if isPopup}
	<ProxyPopup />
{:else}
	<!-- Окно без нативной рамки: заголовок, вкладки и кнопки управления — одна
		 полоса в 32px. Строка алертов между ней и контентом либо занимает ровно
		 24px, либо не существует, поэтому контент никогда не съезжает. -->
	<div class="shell">
		<TitleBar {tab} ontab={goto} />

		<!-- Обёртка обязательна: без алертов AlertStrip не рисует ни одного узла, и
			 тогда строки сетки съезжают — статус-строка попадает в растягивающийся
			 ряд вместо нижнего. Пустой div честно занимает свои 0px. -->
		<div class="alerts">
			<AlertStrip ongoto={goto} />
		</div>

		<main>
			{#if tab === 'dashboard'}
				<Dashboard ongoto={goto} />
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

		<StatusBar />
	</div>

	<!-- Онбординг первого запуска: поверх всего, пока не выбраны бинарник и конфиг. -->
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
</style>
