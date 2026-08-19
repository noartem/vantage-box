// Thin wrapper over Tauri commands and events. All typing lives here, so
// components do not deal with command name strings.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
	ApplyOutcome,
	BinaryInfo,
	CheckResult,
	ConnectionStatus,
	ConnectionsSnapshot,
	InstallOutcome,
	LogEntry,
	Memory,
	ProxyOverview,
	ReleaseCatalog,
	RestartOutcome,
	RuntimeConfigView,
	RunStatus,
	Settings,
	SettingsFileView,
	SubscriptionsState,
	Traffic
} from './types';

export const api = {
	getSettings: () => invoke<Settings>('get_settings'),
	getSettingsPath: () => invoke<string>('get_settings_path'),
	saveSettings: (settings: Settings) => invoke<Settings>('save_settings', { settings }),
	/** The raw settings.json for the built-in editor. */
	readSettingsFile: () => invoke<SettingsFileView>('read_settings_file'),
	/** Parses JSONC and writes through the store (emits settings://changed). */
	writeSettingsFile: (content: string) => invoke<Settings>('write_settings_file', { content }),

	getStatus: () => invoke<ConnectionStatus>('get_status'),

	getProxies: () => invoke<ProxyOverview>('get_proxies'),
	selectProxy: (group: string, name: string) => invoke<void>('select_proxy', { group, name }),
	testGroupDelay: (group: string) => invoke<Record<string, number>>('test_group_delay', { group }),
	testProxyDelay: (name: string) => invoke<number>('test_proxy_delay', { name }),

	getConnections: () => invoke<ConnectionsSnapshot>('get_connections'),
	closeConnection: (id: string) => invoke<void>('close_connection', { id }),
	closeAllConnections: () => invoke<void>('close_all_connections'),

	readSingboxConfig: () => invoke<string>('read_singbox_config'),
	checkSingboxConfig: (content: string) => invoke<CheckResult>('check_singbox_config', { content }),
	writeSingboxConfig: (content: string) => invoke<void>('write_singbox_config', { content }),
	createMinimalConfig: () => invoke<string>('create_minimal_config'),
	/** The runtime config sing-box was last started with (runtime.json). */
	readRuntimeConfig: () => invoke<RuntimeConfigView>('read_runtime_config'),

	getRunStatus: () => invoke<RunStatus>('get_run_status'),
	installService: () => invoke<RunStatus>('install_service'),
	uninstallService: () => invoke<RunStatus>('uninstall_service'),
	start: () => invoke<RunStatus>('start_service'),
	stop: () => invoke<RunStatus>('stop_service'),
	restart: () => invoke<RestartOutcome>('restart_service'),

	getHotkeyProblems: () => invoke<string[]>('get_hotkey_problems'),
	closePopup: () => invoke<void>('close_popup'),
	showMainWindow: () => invoke<void>('show_main_window'),

	generateSecret: () => invoke<string>('generate_secret'),
	/** null — the user closed the dialog. */
	pickFile: (kind: 'config' | 'binary') => invoke<string | null>('pick_file', { kind }),

	getBinaryInfo: () => invoke<BinaryInfo>('get_binary_info'),
	listSingboxReleases: (refresh = false) =>
		invoke<ReleaseCatalog>('list_singbox_releases', { refresh }),
	downloadSingboxRelease: (version: string, assetUrl: string) =>
		invoke<ReleaseCatalog>('download_singbox_release', { version, assetUrl }),
	deleteSingboxRelease: (version: string) =>
		invoke<ReleaseCatalog>('delete_singbox_release', { version }),
	useSingboxRelease: (version: string) =>
		invoke<InstallOutcome>('use_singbox_release', { version }),

	refreshSubscriptions: (force = false) =>
		invoke<ApplyOutcome>('refresh_subscriptions', { force }),
	getSubscriptionState: () => invoke<SubscriptionsState>('get_subscription_state')
};

export const events = {
	status: (fn: (value: ConnectionStatus) => void) => on('clash://status', fn),
	traffic: (fn: (value: Traffic) => void) => on('clash://traffic', fn),
	memory: (fn: (value: Memory) => void) => on('clash://memory', fn),
	connections: (fn: (value: ConnectionsSnapshot) => void) => on('clash://connections', fn),
	log: (fn: (value: LogEntry) => void) => on('clash://log', fn),
	settingsChanged: (fn: (value: Settings) => void) => on('settings://changed', fn),
	settingsError: (fn: (value: string) => void) => on('settings://error', fn),
	configChanged: (fn: (path: string) => void) => on('singbox://config-changed', fn),
	/** sing-box started/stopped — including from the tray or via a hotkey. */
	runStatus: (fn: (value: RunStatus) => void) => on('service://changed', fn),
	/** Selection in a selector group changed — including from the tray or the popup. */
	proxiesChanged: (fn: () => void) => on('proxies://changed', fn),
	hotkeyProblems: (fn: (value: string[]) => void) => on('hotkeys://problems', fn)
};

function on<T>(name: string, fn: (value: T) => void): Promise<UnlistenFn> {
	return listen<T>(name, (event) => fn(event.payload));
}

/** Errors from Rust arrive as strings; everything else is reduced to a readable form. */
export function errorText(error: unknown): string {
	if (typeof error === 'string') return error;
	if (error instanceof Error) return error.message;
	return String(error);
}
