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

	/** Вкладка переживает перезапуск: приложение открывают ради того, на чём
	 *  остановились, а не ради дашборда. */
	const initialTab = loadTab();
	let tab = $state<TabId>(initialTab);
	/** Какие вкладки уже открывали. Компонент монтируется при первом открытии и
	 *  остаётся в DOM до закрытия окна — поэтому повторные переходы не мигают
	 *  пустой загрузкой и не сбрасывают состояние (фильтры, черновики, прокрутку). */
	let opened = $state<Record<string, boolean>>({ [initialTab]: true });

	function goto(next: TabId) {
		tab = next;
		saveTab(next);
		opened[next] = true;
	}

	/** Ctrl+1…7 — вкладки по порядку, как в браузере. Ctrl+Tab / Ctrl+Shift+Tab —
	 *  циклический переход по вкладкам вперёд и назад. Ctrl+Alt+S — настройки.
	 *  Запись хоткея в настройках перехватывает нажатие раньше нас и гасит его —
	 *  уважаем это. */
	function onKeydown(event: KeyboardEvent) {
		if (isPopup || event.defaultPrevented) return;
		if (!event.ctrlKey) return;

		// Ctrl+Alt+S — настройки. Проверяем до общего отсева по Alt, иначе этот
		//  хоткей никогда не дойдёт до дела.
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

		// Ctrl+Shift+W — закрыть окно. close() идёт через CloseRequested, поэтому
		// настройка «сворачивать в трей» продолжает работать.
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
			{#each TABS as t (t.id)}
				{#if opened[t.id]}
					<!-- Не {#if tab === ...}: иначе переключение разрушает вкладку и
						 строит заново — отсюда мигание пустым и потеря состояния.
						 Скрываем CSS-классом, узлы остаются живыми. -->
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
							<ServiceView />
						{:else if t.id === 'settings'}
							<SettingsView active={tab === 'settings'} />
						{/if}
					</div>
				{/if}
			{/each}
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

	/* Каждая вкладка-панель заполняет main; скрытые убраны из раскладки, но узлы
	   живы. height: 100% нужен видам с собственной прокруткой (соединения, логи). */
	.panel {
		height: 100%;
		min-height: 0;
	}

	.panel.hidden {
		display: none;
	}
</style>
