<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { app } from '$lib/state.svelte';
	import type { Settings } from '$lib/types';

	let draft = $state<Settings | null>(null);
	let error = $state<string | null>(null);
	let saving = $state(false);
	/** Secret по умолчанию скрыт: он даёт полное управление sing-box. */
	let secretVisible = $state(false);
	let copied = $state(false);
	/** Какой хоткей сейчас записывается с клавиатуры. */
	let recording = $state<'proxyPopup' | 'toggle' | null>(null);

	$effect(() => {
		// settings.json — источник правды. Правки в файле снаружи перебивают
		// незасейвленную форму: иначе UI показывал бы то, чего в системе нет.
		const current = app.settings;
		if (current) draft = structuredClone($state.snapshot(current)) as Settings;
	});

	const dirty = $derived(
		draft !== null &&
			app.settings !== null &&
			JSON.stringify($state.snapshot(draft)) !== JSON.stringify($state.snapshot(app.settings))
	);

	async function save() {
		if (!draft) return;
		saving = true;
		error = null;
		try {
			await app.saveSettings($state.snapshot(draft) as Settings);
		} catch (e) {
			error = errorText(e);
		} finally {
			saving = false;
		}
	}

	async function openInEditor() {
		error = null;
		try {
			await openPath(app.settingsPath);
		} catch (e) {
			error = errorText(e);
		}
	}

	async function reveal() {
		error = null;
		try {
			await revealItemInDir(app.settingsPath);
		} catch (e) {
			error = errorText(e);
		}
	}

	async function pick(kind: 'config' | 'binary') {
		error = null;
		try {
			const path = await api.pickFile(kind);
			if (!path || !draft) return;
			if (kind === 'config') draft.singBox.configPath = path;
			else draft.singBox.binaryPath = path;
		} catch (e) {
			error = errorText(e);
		}
	}

	async function newSecret() {
		error = null;
		try {
			if (!draft) return;
			draft.clashApi.secret = await api.generateSecret();
			secretVisible = true;
		} catch (e) {
			error = errorText(e);
		}
	}

	async function copySecret() {
		if (!draft?.clashApi.secret) return;
		try {
			await navigator.clipboard.writeText(draft.clashApi.secret);
			copied = true;
			setTimeout(() => (copied = false), 1500);
		} catch (e) {
			error = errorText(e);
		}
	}

	// -------------------------------------------------------------------------
	// Запись хоткеев
	// -------------------------------------------------------------------------

	/** Клавиши, которые в акселераторе называются не так, как `KeyboardEvent.code`. */
	const KEY_NAMES: Record<string, string> = {
		Escape: 'Esc',
		Backquote: '`',
		Minus: '-',
		Equal: '=',
		BracketLeft: '[',
		BracketRight: ']',
		Backslash: '\\',
		Semicolon: ';',
		Quote: "'",
		Comma: ',',
		Period: '.',
		Slash: '/'
	};

	/** Основная клавиша комбинации. `null` — нажат только модификатор. */
	function mainKey(code: string): string | null {
		if (/^(Control|Alt|Shift|Meta|OS)/.test(code)) return null;
		const letter = /^Key([A-Z])$/.exec(code);
		if (letter) return letter[1];
		const digit = /^Digit(\d)$/.exec(code);
		if (digit) return digit[1];
		const numpad = /^Numpad(\d)$/.exec(code);
		if (numpad) return `Numpad${numpad[1]}`;
		return KEY_NAMES[code] ?? code;
	}

	function onKeydown(event: KeyboardEvent) {
		if (!recording || !draft) return;
		event.preventDefault();
		event.stopPropagation();

		// Esc выходит из записи, не назначая себя: иначе выйти было бы нечем.
		if (event.code === 'Escape') {
			recording = null;
			return;
		}

		const key = mainKey(event.code);
		if (key === null) return;

		const mods: string[] = [];
		if (event.ctrlKey) mods.push('Ctrl');
		if (event.altKey) mods.push('Alt');
		if (event.shiftKey) mods.push('Shift');
		if (event.metaKey) mods.push('Super');

		// Глобальный хоткей без модификатора отобрал бы клавишу у всей системы.
		if (mods.length === 0) return;

		draft.hotkeys[recording] = [...mods, key].join('+');
		recording = null;
	}

	function clearHotkey(name: 'proxyPopup' | 'toggle') {
		if (!draft) return;
		draft.hotkeys[name] = '';
		recording = null;
	}
</script>

<svelte:window onkeydown={onKeydown} />

<div class="page">
	{#if draft}
		{#if error}
			<div class="banner">{error}</div>
		{/if}

		<section class="card">
			<h3>Clash API</h3>
			<label>
				<span>Адрес</span>
				<input bind:value={draft.clashApi.url} placeholder="http://127.0.0.1:9797" />
			</label>
			<label>
				<span>Secret</span>
				<div class="field">
					<input
						type={secretVisible ? 'text' : 'password'}
						bind:value={draft.clashApi.secret}
						placeholder="пусто — сгенерируем на каждый запуск"
					/>
					<button onclick={() => (secretVisible = !secretVisible)}>
						{secretVisible ? 'Скрыть' : 'Показать'}
					</button>
					{#if secretVisible}
						<button onclick={copySecret} disabled={!draft.clashApi.secret}>
							{copied ? 'Скопировано' : 'Копировать'}
						</button>
					{/if}
					<button onclick={newSecret}>Сгенерировать</button>
				</div>
			</label>
			<label>
				<span>Уровень логов</span>
				<select bind:value={draft.clashApi.logLevel}>
					{#each ['trace', 'debug', 'info', 'warn', 'error'] as level (level)}
						<option value={level}>{level}</option>
					{/each}
				</select>
			</label>
			<p class="muted hint">
				Изменение адреса, secret'а или уровня логов сразу переоткрывает подписки — перезапуск
				приложения не нужен. Пустой secret означает, что Vantage Box выдаст свой на каждый запуск
				sing-box.
			</p>
		</section>

		<section class="card">
			<h3>sing-box</h3>
			<label>
				<span>Конфиг (JSON-файл)</span>
				<div class="field">
					<input bind:value={draft.singBox.configPath} placeholder="путь к config sing-box" />
					<button onclick={() => pick('config')}>Выбрать…</button>
				</div>
			</label>
			<label>
				<span>Файл sing-box</span>
				<div class="field">
					<input
						bind:value={draft.singBox.binaryPath}
						placeholder="пусто — файл под управлением Vantage Box"
					/>
					<button onclick={() => pick('binary')}>Выбрать…</button>
				</div>
			</label>
			<label>
				<span>Обновление sing-box</span>
				<select bind:value={draft.singBox.updatePolicy}>
					<option value="off">не проверять</option>
					<option value="notify">уведомлять</option>
					<option value="auto">ставить автоматически</option>
				</select>
			</label>
		</section>

		<section class="card">
			<h3>Интерфейс</h3>
			<label>
				<span>Тема</span>
				<select bind:value={draft.ui.theme}>
					<option value="system">как в системе</option>
					<option value="light">светлая</option>
					<option value="dark">тёмная</option>
				</select>
			</label>
			<label>
				<span>URL latency-теста</span>
				<input bind:value={draft.ui.latencyTestUrl} />
			</label>
			<label>
				<span>Таймаут теста, мс</span>
				<input type="number" min="100" max="60000" bind:value={draft.ui.latencyTestTimeout} />
			</label>
		</section>

		<section class="card">
			<h3>Трей и запуск</h3>
			<label class="row">
				<input type="checkbox" bind:checked={draft.tray.enabled} />
				<span>Показывать иконку в трее</span>
			</label>
			<label class="row">
				<input type="checkbox" bind:checked={draft.tray.closeToTray} />
				<span>Закрытие окна сворачивает в трей</span>
			</label>
			<label class="row">
				<input type="checkbox" bind:checked={draft.tray.startMinimized} />
				<span>Запускать свёрнутым</span>
			</label>
			<label class="row">
				<input type="checkbox" bind:checked={draft.autostart} />
				<span>Запускать при входе в систему</span>
			</label>
			<p class="muted hint">
				Иконка в трее и запуск свёрнутым появляются после перезапуска приложения — трей создаётся
				один раз при старте.
			</p>
		</section>

		<section class="card">
			<h3>Хоткеи</h3>
			{#each [{ id: 'proxyPopup', label: 'Попап выбора прокси' }, { id: 'toggle', label: 'Включить / выключить' }] as item (item.id)}
				{@const name = item.id as 'proxyPopup' | 'toggle'}
				<label>
					<span>{item.label}</span>
					<div class="field">
						<input
							bind:value={draft.hotkeys[name]}
							placeholder="пусто — без хоткея"
							readonly={recording === name}
						/>
						<button
							class:primary={recording === name}
							onclick={() => (recording = recording === name ? null : name)}
						>
							{recording === name ? 'Нажмите…' : 'Записать'}
						</button>
						<button onclick={() => clearHotkey(name)} disabled={draft.hotkeys[name] === ''}>
							Очистить
						</button>
					</div>
				</label>
			{/each}
			<p class="muted hint">
				«Записать» — и нажмите комбинацию целиком. Модификаторы: <code>Ctrl</code>,
				<code>Alt</code>, <code>Shift</code>, <code>Super</code> (клавиша Windows). Нужен хотя бы
				один: без модификатора хоткей отобрал бы клавишу у всей системы.
				<code>Esc</code> отменяет запись. Комбинацию можно и просто вписать руками, через
				<code>+</code>. Хоткеи работают глобально, даже когда окно закрыто.
			</p>
			{#if app.hotkeyProblems.length > 0}
				<div class="banner">
					<strong>Не удалось занять комбинации:</strong>
					<ul>
						{#each app.hotkeyProblems as problem (problem)}
							<li>{problem}</li>
						{/each}
					</ul>
				</div>
			{/if}
		</section>

		<!-- Файл настроек — для тех, кто правит его руками. Рядовому пользователю
			 он не нужен, поэтому лежит в самом низу. -->
		<section class="card">
			<h3>Файл настроек</h3>
			<code class="path selectable">{app.settingsPath}</code>
			<div class="actions">
				<button onclick={openInEditor}>Открыть</button>
				<button onclick={reveal}>Показать в папке</button>
			</div>
			<p class="muted hint">
				Файл читается как JSONC — комментарии и висячие запятые допустимы. Правки подхватываются
				на лету, перезапуск не нужен.
			</p>
		</section>

		<div class="footer">
			<button class="primary" onclick={save} disabled={!dirty || saving}>
				{saving ? 'Сохраняю…' : 'Сохранить'}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving}>Отменить</button>
			{#if dirty}<span class="muted">есть несохранённые изменения</span>{/if}
		</div>
	{:else}
		<p class="muted">Загружаю настройки…</p>
	{/if}
</div>

<style>
	.page {
		display: grid;
		gap: 12px;
		align-content: start;
		max-width: 680px;
	}

	section {
		padding: 14px;
		display: grid;
		gap: 10px;
	}

	h3 {
		font-size: 14px;
	}

	label {
		display: grid;
		grid-template-columns: 180px 1fr;
		align-items: center;
		gap: 10px;
	}

	label.row {
		grid-template-columns: auto 1fr;
		justify-items: start;
	}

	/* Инпут с кнопками справа: кнопки по содержимому, поле забирает остаток. */
	.field {
		display: flex;
		gap: 6px;
		min-width: 0;
	}

	.field button {
		flex-shrink: 0;
	}

	.path {
		font-family: var(--mono);
		font-size: 12px;
		word-break: break-all;
	}

	.actions {
		display: flex;
		gap: 8px;
	}

	.hint {
		margin: 0;
		font-size: 12px;
	}

	.hint code {
		font-family: var(--mono);
		background: var(--surface-alt);
		padding: 1px 4px;
		border-radius: 4px;
	}

	.banner ul {
		margin: 4px 0 0;
		padding-left: 18px;
	}

	.footer {
		display: flex;
		align-items: center;
		gap: 10px;
		position: sticky;
		bottom: 0;
		padding: 10px 0;
		background: var(--bg);
	}
</style>
