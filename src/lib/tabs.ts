import type { IconName } from './icons';

/** Окно приложения — не браузер: вкладки держим в состоянии, без роутинга и URL.
 *  Список вынесен из +page.svelte, потому что на него ссылается и строка алертов
 *  (её кнопки уводят на нужную вкладку). */
export const TABS = [
	{ id: 'dashboard', label: 'Дашборд', icon: 'dashboard' },
	{ id: 'connections', label: 'Соединения', icon: 'connections' },
	{ id: 'subscriptions', label: 'Подписки', icon: 'subscriptions' },
	{ id: 'config', label: 'Конфиг', icon: 'config' },
	{ id: 'logs', label: 'Логи', icon: 'logs' },
	{ id: 'service', label: 'Сервис', icon: 'service' },
	{ id: 'settings', label: 'Настройки', icon: 'settings' }
] as const satisfies readonly { id: string; label: string; icon: IconName }[];

export type TabId = (typeof TABS)[number]['id'];

const STORAGE_KEY = 'vb.tab';

/** Вкладка переживает перезапуск: приложение открывают ради того, на чём
 *  остановились, а не ради дашборда. */
export function loadTab(): TabId {
	if (typeof localStorage === 'undefined') return 'dashboard';
	const saved = localStorage.getItem(STORAGE_KEY);
	return TABS.some((t) => t.id === saved) ? (saved as TabId) : 'dashboard';
}

export function saveTab(tab: TabId) {
	if (typeof localStorage === 'undefined') return;
	localStorage.setItem(STORAGE_KEY, tab);
}
