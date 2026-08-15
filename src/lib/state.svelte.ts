// Разделяемое состояние UI. Ничего, кроме буферов и последних снимков, здесь
// не живёт: источник правды — sing-box и settings.json.

import { api, errorText, events } from './api';
import { check as checkForTauriUpdate, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import type {
	BinaryInfo,
	Connection,
	ConnectionStatus,
	LogEntry,
	Memory,
	RunStatus,
	Settings,
	Theme,
	Traffic
} from './types';

/** Сколько точек трафика держим для графика — ровно минута при 1 Гц. */
const TRAFFIC_POINTS = 60;
/** Потолок ленты логов. Ring buffer, чтобы не съесть память за сутки аптайма. */
const LOG_LIMIT = 2000;
/** Как часто перечитываем состояние sing-box, если событий не приходило. */
const RUN_STATUS_MS = 5000;
/** Как часто перепроверяем обновления приложения, если окно долго открыто. */
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
	/** Последняя проблема с settings.json — показываем баннером. */
	settingsProblem = $state<string | null>(null);

	traffic = $state<Traffic>({ up: 0, down: 0 });
	trafficHistory = $state<Traffic[]>([]);
	/** Накопленный объём за текущий сеанс sing-box. */
	totals = $state<Traffic>({ up: 0, down: 0 });
	memory = $state<Memory>({ inuse: 0, oslimit: 0 });

	/** Активные соединения sing-box. Источник — WS `/connections`. */
	connections = $state<Connection[]>([]);
	connectionTotals = $state<{ down: number; up: number }>({ down: 0, up: 0 });

	/** Доступное обновление приложения (режим `notify`) — показываем баннером. */
	updateAvailable = $state<{ version: string; body?: string } | null>(null);
	/** Установка обновления в процессе — кнопка становится неактивной. */
	updateInstalling = $state(false);
	/** Текущее скачиваемое обновление — нужно, чтобы доустановить по клику. */
	private pendingUpdate: Update | null = null;

	/** Состояние sing-box: сервис или процесс, работает или нет. */
	run = $state<RunStatus | null>(null);

	/** Сведения о файле sing-box — нужен онбордингу, чтобы понять, есть ли бинарник. */
	binaryInfo = $state<BinaryInfo | null>(null);

	/** Показывать онбординг первого запуска: нет конфига или нет бинарника.
	 *  Пока настройки/бинарник не загружены — false, чтобы не мигать оверлеем. */
	needsOnboarding = $derived(
		this.settings !== null &&
			(this.settings.singBox.configPath.trim() === '' ||
				(this.binaryInfo !== null && !this.binaryInfo.present))
	);

	/** Путь к config.json, если его изменили в обход приложения. */
	configChangedExternally = $state<string | null>(null);
	/** Хоткеи, которые не удалось зарегистрировать. */
	hotkeyProblems = $state<string[]>([]);

	logs = $state<LogEntry[]>([]);
	logsPaused = $state(false);
	/** Пока лента на паузе, записи копятся здесь и не теряются. */
	private pendingLogs: LogEntry[] = [];

	private started = false;

	/** Подписки на события бэкенда. Вызывается один раз из layout. */
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
			// Политика обновлений могла измениться — перепроверим.
			this.checkForAppUpdate();
			// Путь к бинарнику мог измениться — перечитаем сведения о нём.
			this.refreshBinaryInfo();
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

		// Обновления приложения: проверяем по политике из настроек. Ошибки сети
		// или подписи сюда не поднимаем — пользователь не виноват, что эндпоинт
		// недоступен, и баннер с ошибкой только напугает.
		this.checkForAppUpdate();
		setInterval(() => this.checkForAppUpdate(), UPDATE_RECHECK_MS);

		// Сервисом можно управлять и снаружи приложения, поэтому одних событий
		// мало: состояние надо перечитывать.
		this.refreshRun();
		setInterval(() => this.refreshRun(), RUN_STATUS_MS);

		// Бинарник sing-box нужен онбордингу — подтянем сразу.
		this.refreshBinaryInfo();
	}

	/** Проверяет обновление приложения согласно `settings.guiUpdate.policy`. */
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
			// notify — запоминаем и показываем баннер с кнопкой «установить».
			this.pendingUpdate = update;
			this.updateAvailable = { version: update.version, body: update.body };
		} catch {
			// См. комментарий выше — молча.
		}
	}

	/** Установить отложенное обновление (режим `notify`): скачать и перезапустить. */
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
			// Единственный источник — локальный диспетчер сервисов. Если он не
			// отвечает, следующий опрос через несколько секунд всё исправит.
		}
	}

	async refreshBinaryInfo() {
		try {
			this.binaryInfo = await api.getBinaryInfo();
		} catch {
			// Не критично: онбординг просто не покажет шаг про бинарник.
		}
	}

	/** Счётчики трафика принадлежат сеансу sing-box, а не окну. */
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
		// Clash отдаёт «за последнюю секунду», поэтому суммой получается объём.
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

/** Тема применяется атрибутом на <html>: CSS дальше разбирается сам. */
export function applyTheme(theme: Theme) {
	if (typeof document === 'undefined') return;
	document.documentElement.dataset.theme = theme;
}

export const app = new AppState();
