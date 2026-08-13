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
