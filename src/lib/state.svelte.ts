// Shared UI state. Nothing but buffers and latest snapshots lives here: the source
// of truth is sing-box and settings.json.

import { api, errorText, events } from './api';
import { check as checkForTauriUpdate, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import type {
	BinaryInfo,
	Connection,
	ConnectionStatus,
	LogEntry,
	Memory,
	ReleaseCatalog,
	RunStatus,
	Settings,
	Theme,
	Traffic
} from './types';

/** How many traffic points we keep for the chart — exactly one minute at 1 Hz. */
const TRAFFIC_POINTS = 60;
/** Log feed cap. Ring buffer, so we do not eat memory over a day of uptime. */
const LOG_LIMIT = 2000;
/** How often we re-read sing-box state if no events have arrived. */
const RUN_STATUS_MS = 5000;
/** How often we re-check for app updates if the window stays open a long time. */
const UPDATE_RECHECK_MS = 6 * 60 * 60 * 1000;

class AppState {
	status = $state<ConnectionStatus>({
		state: 'connecting',
		version: null,
		error: null,
		compatibility: 'unknown'
	});

	settings = $state<Settings | null>(null);
	settingsPath = $state('');
	/** Last problem with settings.json — shown as a banner. */
	settingsProblem = $state<string | null>(null);

	traffic = $state<Traffic>({ up: 0, down: 0 });
	trafficHistory = $state<Traffic[]>([]);
	/** Accumulated volume over the current sing-box session. */
	totals = $state<Traffic>({ up: 0, down: 0 });
	memory = $state<Memory>({ inuse: 0, oslimit: 0 });

	/** Active sing-box connections. Source is the WS `/connections` endpoint. */
	connections = $state<Connection[]>([]);
	connectionTotals = $state<{ down: number; up: number }>({ down: 0, up: 0 });

	/** Available app update (`notify` mode) — shown as a banner. */
	updateAvailable = $state<{ version: string; body?: string } | null>(null);
	/** An update install is in progress — the button becomes disabled. */
	updateInstalling = $state(false);
	/** The update currently being downloaded — needed to finish installing it on click. */
	private pendingUpdate: Update | null = null;

	/** sing-box state: service or process, running or not. */
	run = $state<RunStatus | null>(null);

	/** Details about the sing-box file — onboarding needs it to tell whether the binary exists. */
	binaryInfo = $state<BinaryInfo | null>(null);

	/** sing-box release catalog. Lives in shared state so the "Service" tab opens
	 *  without flickering: we preload it on startup and keep it around. */
	catalog = $state<ReleaseCatalog | null>(null);
	catalogRefreshing = $state(false);

	/** Whether to show first-run onboarding: no config or no binary.
	 *  While settings/binary are not loaded — false, so the overlay does not flash. */
	needsOnboarding = $derived(
		this.settings !== null &&
			(this.settings.singBox.configPath.trim() === '' ||
				(this.binaryInfo !== null && !this.binaryInfo.present))
	);

	/** Path to config.json, if it was changed outside the app. */
	configChangedExternally = $state<string | null>(null);
	/** Hotkeys that failed to register. */
	hotkeyProblems = $state<string[]>([]);

	logs = $state<LogEntry[]>([]);
	logsPaused = $state(false);
	/** While the feed is paused, entries accumulate here and are not lost. */
	private pendingLogs: LogEntry[] = [];

	private started = false;

	/** Subscriptions to backend events. Called once from the layout. */
	async start() {
		if (this.started) return;
		this.started = true;

		events.status((value) => (this.status = value));
		events.traffic((value) => this.pushTraffic(value));
		events.memory((value) => (this.memory = value));
		events.connections((value) => {
			this.connections = value.connections;
			this.connectionTotals = { down: value.downloadTotal, up: value.uploadTotal };
		});
		events.log((value) => this.pushLog(value));
		events.settingsChanged((value) => {
			this.settings = value;
			this.settingsProblem = null;
			applyTheme(value.ui.theme);
			// The update policy may have changed — re-check.
			this.checkForAppUpdate();
			// The binary path may have changed — re-read its details.
			this.refreshBinaryInfo();
			// Along with it, which version is considered active may also change.
			this.refreshCatalog().catch(() => {});
		});
		events.settingsError((value) => (this.settingsProblem = value));
		events.configChanged((path) => (this.configChangedExternally = path));
		events.hotkeyProblems((value) => (this.hotkeyProblems = value));
		events.runStatus((value) => this.setRun(value));

		await this.refreshSettings();
		try {
			this.status = await api.getStatus();
			this.hotkeyProblems = await api.getHotkeyProblems();
		} catch (e) {
			this.settingsProblem = errorText(e);
		}

		// App updates: check according to the policy from settings. We do not
		// surface network or signature errors here — the user is not at fault for
		// an unreachable endpoint, and an error banner would only alarm them.
		this.checkForAppUpdate();
		setInterval(() => this.checkForAppUpdate(), UPDATE_RECHECK_MS);

		// The service can also be controlled from outside the app, so events alone
		// are not enough: the state must be re-read.
		this.refreshRun();
		setInterval(() => this.refreshRun(), RUN_STATUS_MS);

		// The sing-box binary is needed by onboarding — load it right away.
		this.refreshBinaryInfo();

		// Preload the release catalog from the backend cache (without hitting
		// GitHub) so the first opening of the "Service" tab does not flash a loader.
		this.refreshCatalog().catch(() => {
			// The backend cache may not exist yet — the tab will load it itself.
		});
	}

	/** Checks for an app update per `settings.guiUpdate.policy`. */
	async checkForAppUpdate() {
		const policy = this.settings?.guiUpdate.policy;
		if (!policy || policy === 'off') {
			this.updateAvailable = null;
			this.pendingUpdate = null;
			return;
		}
		try {
			const update = await checkForTauriUpdate();
			if (!update) {
				this.updateAvailable = null;
				this.pendingUpdate = null;
				return;
			}
			if (policy === 'auto') {
				await update.downloadAndInstall();
				await relaunch();
				return;
			}
			// notify — remember it and show a banner with an "install" button.
			this.pendingUpdate = update;
			this.updateAvailable = { version: update.version, body: update.body };
		} catch {
			// See the comment above — stay silent.
		}
	}

	/** Install the deferred update (`notify` mode): download and restart. */
	async installAppUpdate() {
		if (!this.pendingUpdate || this.updateInstalling) return;
		this.updateInstalling = true;
		try {
			await this.pendingUpdate.downloadAndInstall();
			await relaunch();
		} catch (e) {
			this.settingsProblem = errorText(e);
		} finally {
			this.updateInstalling = false;
		}
	}

	async refreshRun() {
		try {
			this.setRun(await api.getRunStatus());
		} catch {
			// The only source is the local service dispatcher. If it does not
			// respond, the next poll a few seconds later will sort it out.
		}
	}

	async refreshBinaryInfo() {
		try {
			this.binaryInfo = await api.getBinaryInfo();
		} catch {
			// Not critical: onboarding simply will not show the binary step.
		}
	}

	/** Re-read the sing-box release catalog. `refresh=true` — go to GitHub,
	 *  otherwise read the backend cache (fast, no network). Errors are propagated:
	 *  the startup preload silences them itself, while the "Service" tab shows a banner. */
	async refreshCatalog(refresh = false) {
		if (refresh) this.catalogRefreshing = true;
		try {
			this.catalog = await api.listSingboxReleases(refresh);
		} finally {
			this.catalogRefreshing = false;
		}
	}

	/** Traffic counters belong to the sing-box session, not the window. */
	private setRun(value: RunStatus) {
		if (value.running && this.run?.running === false) {
			this.totals = { up: 0, down: 0 };
		}
		this.run = value;
	}

	async refreshSettings() {
		try {
			this.settings = await api.getSettings();
			this.settingsPath = await api.getSettingsPath();
			applyTheme(this.settings.ui.theme);
		} catch (e) {
			this.settingsProblem = errorText(e);
		}
	}

	async saveSettings(next: Settings) {
		this.settings = await api.saveSettings(next);
		this.settingsProblem = null;
		applyTheme(this.settings.ui.theme);
	}

	setLogsPaused(paused: boolean) {
		this.logsPaused = paused;
		if (!paused && this.pendingLogs.length > 0) {
			this.logs = trim([...this.logs, ...this.pendingLogs], LOG_LIMIT);
			this.pendingLogs = [];
		}
	}

	clearLogs() {
		this.logs = [];
		this.pendingLogs = [];
	}

	private pushTraffic(value: Traffic) {
		this.traffic = value;
		this.trafficHistory = trim([...this.trafficHistory, value], TRAFFIC_POINTS);
		// Clash reports "over the last second", so summing yields a volume.
		this.totals = {
			up: this.totals.up + value.up,
			down: this.totals.down + value.down
		};
	}

	private pushLog(value: LogEntry) {
		if (this.logsPaused) {
			this.pendingLogs = trim([...this.pendingLogs, value], LOG_LIMIT);
			return;
		}
		this.logs = trim([...this.logs, value], LOG_LIMIT);
	}
}

function trim<T>(items: T[], limit: number): T[] {
	return items.length > limit ? items.slice(items.length - limit) : items;
}

/** The theme is applied via an attribute on <html>: the CSS handles the rest. */
export function applyTheme(theme: Theme) {
	if (typeof document === 'undefined') return;
	document.documentElement.dataset.theme = theme;
}

export const app = new AppState();
