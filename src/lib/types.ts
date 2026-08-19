// Mirror of the Rust models. Kept in sync with src-tauri/src/settings.rs
// and src-tauri/src/clash/models.rs.

export type UpdatePolicy = 'off' | 'notify' | 'auto';
export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';
export type Theme = 'system' | 'light' | 'dark';

export interface Settings {
	$schema?: string;
	singBox: {
		configPath: string;
		binaryPath: string;
		updatePolicy: UpdatePolicy;
	};
	clashApi: {
		url: string;
		secret: string;
		logLevel: LogLevel;
	};
	ui: {
		theme: Theme;
		latencyTestUrl: string;
		latencyTestTimeout: number;
	};
	tray: {
		enabled: boolean;
		closeToTray: boolean;
		startMinimized: boolean;
	};
	hotkeys: {
		proxyPopup: string;
		toggle: string;
	};
	autostart: boolean;
	guiUpdate: {
		policy: UpdatePolicy;
	};
	subscriptions: SubscriptionSettings[];
	fallback: FallbackSettings;
}

export interface SubscriptionSettings {
	id: string;
	name: string;
	url: string;
	enabled: boolean;
	/** Tag of the selector group to pour nodes into. null — into all selector/urltest groups. */
	targetGroup: string | null;
	/** How often to pull the subscription, in hours. */
	updateInterval: number;
}

export interface FallbackSettings {
	enabled: boolean;
	intervalSec: number;
	timeoutMs: number;
	maxDelayMs: number;
	/** Tags of groups to track. Empty — all selector groups. */
	groups: string[];
}

/** Summary of a single subscription after an update. */
export interface SubUpdate {
	id: string;
	name: string;
	/** How many nodes were poured in. */
	nodeCount: number;
	/** Unix ms of the last update. */
	lastUpdated: number;
	lastError: string | null;
}

export interface ApplyOutcome {
	updates: SubUpdate[];
	changed: boolean;
	restarted: boolean;
}

/** State of a single subscription from the sidecar file (for the UI). */
export interface SubStateEntry {
	lastUpdated: number;
	nodeCount: number;
	lastError: string | null;
}

export interface SubscriptionsState {
	entries: Record<string, SubStateEntry>;
	/** Tags of groups subscriptions created (not merely filled). Internal. */
	createdGroups?: string[];
}

export type ConnectionState = 'disconnected' | 'connecting' | 'connected';
export type Compatibility = 'unknown' | 'supported' | 'tooOld' | 'tooNew';

export interface ConnectionStatus {
	state: ConnectionState;
	version: string | null;
	error: string | null;
	compatibility: Compatibility;
}

export interface Traffic {
	up: number;
	down: number;
}

export interface Memory {
	inuse: number;
	oslimit: number;
}

export interface ConnectionMetadata {
	network: string;
	/** Inbound type: `Mixed`, `Tun`, … */
	type: string;
	sourceIP: string;
	sourcePort: string;
	destinationIP: string;
	destinationPort: string;
	/** Destination host of the connection. */
	host: string;
	processPath: string;
}

export interface Connection {
	id: string;
	/** Chain of outbounds: `[node, group]` from the outside in. */
	chains: string[];
	rule: string;
	rulePayload: string;
	/** Network attributes — the `metadata` sub-object in the sing-box response. */
	metadata: ConnectionMetadata;
	upload: number;
	download: number;
	/** ISO time the connection started. */
	start: string;
}

export interface ConnectionsSnapshot {
	downloadTotal: number;
	uploadTotal: number;
	connections: Connection[];
}

export interface LogEntry {
	id: number;
	/** Unix time in milliseconds. */
	time: number;
	level: string;
	message: string;
}

export interface NodeView {
	name: string;
	kind: string;
	delay: number | null;
	udp: boolean;
	isGroup: boolean;
}

export interface GroupView {
	name: string;
	kind: string;
	now: string | null;
	selectable: boolean;
	items: NodeView[];
}

export interface ProxyOverview {
	groups: GroupView[];
}

export type ServiceState =
	| 'notInstalled'
	| 'stopped'
	| 'startPending'
	| 'running'
	| 'stopPending'
	| 'unknown';

export interface ServiceInfo {
	name: string;
	/** Whether a ServiceController implementation exists for the current OS. */
	supported: boolean;
	state: ServiceState;
	/** Whether we have rights to start/stop without UAC. */
	canControl: boolean;
	detail: string | null;
}

/** How sing-box is started: as a service (needed for TUN) or as a regular process. */
export type RunMode = 'service' | 'process';

export interface RunStatus {
	mode: RunMode;
	/** Whether sing-box is running — regardless of service or process. */
	running: boolean;
	service: ServiceInfo;
	/** PID of the child process, when running outside the service. */
	processPid: number | null;
	/** The config needs TUN, which means a service and admin rights. */
	tun: boolean;
	/** Why the config could not be read. */
	configProblem: string | null;
}

export interface BinaryInfo {
	path: string;
	/** Binary is managed by Vantage Box — can be auto-updated. */
	managed: boolean;
	present: boolean;
	version: string | null;
	compatibility: Compatibility;
	problem: string | null;
	supportedRange: string;
}

export interface RestartOutcome {
	status: RunStatus;
	/** Lines of the form "group → node". */
	restored: string[];
	/** What could not be restored, with a reason. */
	skipped: string[];
	apiBack: boolean;
}

export interface ReleaseInfo {
	version: string;
	prerelease: boolean;
	compatibility: Compatibility;
	/** null — no build for the current platform in this release. */
	asset: string | null;
	assetUrl: string | null;
	size: number;
	/** This version's file is already on disk. */
	downloaded: boolean;
	/** This exact version is currently in use. */
	active: boolean;
}

export interface ReleaseCatalog {
	/** When the list was fetched from GitHub, unix time in seconds. 0 — never. */
	fetchedAt: number;
	/** The cache is due for a refresh. */
	stale: boolean;
	releases: ReleaseInfo[];
}

export interface InstallOutcome {
	binary: BinaryInfo;
	restarted: boolean;
	check: CheckResult;
}

export interface CheckResult {
	/** Whether a `sing-box check` was performed. */
	available: boolean;
	ok: boolean;
	output: string;
}

/** The runtime config sing-box was last started with — the user's config.json
 *  plus the injected Clash API block. Read-only, for debugging. */
export interface RuntimeConfigView {
	path: string;
	content: string;
}

/** The raw settings.json for the built-in editor — exactly what is on disk. */
export interface SettingsFileView {
	path: string;
	content: string;
}
