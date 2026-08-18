import { m } from '$lib/paraglide/messages.js';
import type { IconName } from './icons';

/** The app is a window, not a browser: tabs live in state, with no routing or
 *  URLs. The list is kept out of +page.svelte because the alert strip also
 *  references it (its buttons switch to the right tab).
 *
 *  Labels are lazy functions: m.x() reads the locale at call time (render), not
 *  at module load — so they pick up the locale applyLocale() set before render. */
export const TABS = [
	{ id: 'dashboard', label: () => m.tabs_dashboard(), icon: 'dashboard' },
	{ id: 'logs', label: () => m.tabs_logs(), icon: 'logs' },
	{ id: 'connections', label: () => m.tabs_connections(), icon: 'connections' },
	{ id: 'config', label: () => m.tabs_config(), icon: 'config' },
	{ id: 'subscriptions', label: () => m.tabs_subscriptions(), icon: 'subscriptions' },
	{ id: 'service', label: () => m.tabs_service(), icon: 'service' },
	{ id: 'settings', label: () => m.tabs_settings(), icon: 'settings' }
] as const satisfies readonly { id: string; label: () => string; icon: IconName }[];

export type TabId = (typeof TABS)[number]['id'];

const STORAGE_KEY = 'vb.tab';

/** The tab survives a restart: the app is opened to continue where you left
 *  off, not at the dashboard. */
export function loadTab(): TabId {
	if (typeof localStorage === 'undefined') return 'dashboard';
	const saved = localStorage.getItem(STORAGE_KEY);
	return TABS.some((t) => t.id === saved) ? (saved as TabId) : 'dashboard';
}

export function saveTab(tab: TabId) {
	if (typeof localStorage === 'undefined') return;
	localStorage.setItem(STORAGE_KEY, tab);
}