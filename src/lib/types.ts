// Зеркало моделей из Rust. Держим синхронно с src-tauri/src/settings.rs
// и src-tauri/src/clash/models.rs.

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
	/** Тег selector-группы, куда влить узлы. null — во все selector/urltest-группы. */
	targetGroup: string | null;
	/** Как часто перетягивать подписку, часы. */
	updateInterval: number;
}

export interface FallbackSettings {
	enabled: boolean;
	intervalSec: number;
	timeoutMs: number;
	maxDelayMs: number;
	/** Теги групп для слежения. Пусто — все selector-группы. */
	groups: string[];
}

/** Сводка по одной подписке после обновления. */
export interface SubUpdate {
	id: string;
	name: string;
	/** Сколько узлов влито. */
	nodeCount: number;
	/** Unix-мс последнего обновления. */
	lastUpdated: number;
	lastError: string | null;
}

export interface ApplyOutcome {
	updates: SubUpdate[];
	changed: boolean;
	restarted: boolean;
}

/** Состояние одной подписки из sidecar-файла (для UI). */
export interface SubStateEntry {
	lastUpdated: number;
	nodeCount: number;
	lastError: string | null;
}

export interface SubscriptionsState {
	entries: Record<string, SubStateEntry>;
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
	/** Тип инбаунда: `Mixed`, `Tun`, … */
	type: string;
	sourceIP: string;
	sourcePort: string;
	destinationIP: string;
	destinationPort: string;
	/** Целевой хост соединения. */
	host: string;
	processPath: string;
}

export interface Connection {
	id: string;
	/** Цепочка outbound'ов: `[узел, группа]` снаружи внутрь. */
	chains: string[];
	rule: string;
	rulePayload: string;
	/** Сетевые атрибуты — подобъект `metadata` в ответе sing-box. */
	metadata: ConnectionMetadata;
	upload: number;
	download: number;
	/** ISO-время старта соединения. */
	start: string;
}

export interface ConnectionsSnapshot {
	downloadTotal: number;
	uploadTotal: number;
	connections: Connection[];
}

export interface LogEntry {
	id: number;
	/** Unix-время в миллисекундах. */
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
	/** Есть ли реализация ServiceController под текущую ОС. */
	supported: boolean;
	state: ServiceState;
	/** Хватает ли прав на start/stop без UAC. */
	canControl: boolean;
	detail: string | null;
}

/** Как запускается sing-box: сервисом (нужен для TUN) или обычным процессом. */
export type RunMode = 'service' | 'process';

export interface RunStatus {
	mode: RunMode;
	/** Работает ли sing-box — неважно, сервисом или процессом. */
	running: boolean;
	service: ServiceInfo;
	/** PID дочернего процесса, если запуск идёт мимо сервиса. */
	processPid: number | null;
	/** Конфигу нужен TUN, а значит — сервис и права администратора. */
	tun: boolean;
	/** Почему не удалось прочитать конфиг. */
	configProblem: string | null;
}

export interface BinaryInfo {
	path: string;
	/** Бинарник под управлением Vantage Box — можно обновлять автоматически. */
	managed: boolean;
	present: boolean;
	version: string | null;
	compatibility: Compatibility;
	problem: string | null;
	supportedRange: string;
}

export interface RestartOutcome {
	status: RunStatus;
	/** Строки вида «группа → узел». */
	restored: string[];
	/** Что восстановить не удалось, с причиной. */
	skipped: string[];
	apiBack: boolean;
}

export interface ReleaseInfo {
	version: string;
	prerelease: boolean;
	compatibility: Compatibility;
	/** null — сборки под текущую платформу в релизе нет. */
	asset: string | null;
	assetUrl: string | null;
	size: number;
	/** Файл этой версии уже лежит на диске. */
	downloaded: boolean;
	/** Именно эта версия сейчас используется. */
	active: boolean;
}

export interface ReleaseCatalog {
	/** Когда список забирали с GitHub, unix-время в секундах. 0 — никогда. */
	fetchedAt: number;
	/** Кэш пора обновить. */
	stale: boolean;
	releases: ReleaseInfo[];
}

export interface InstallOutcome {
	binary: BinaryInfo;
	restarted: boolean;
	check: CheckResult;
}

export interface CheckResult {
	/** Была ли выполнена проверка через `sing-box check`. */
	available: boolean;
	ok: boolean;
	output: string;
}
