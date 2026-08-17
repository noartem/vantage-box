<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { app } from '$lib/state.svelte';
	import type { Settings } from '$lib/types';

	/** Активна ли вкладка настроек. Вкладки не разрушаются при переключении,
	 *  поэтому сами по себе не знают, видны ли. Нужно, чтобы погасить запись
	 *  хоткея при уходе — иначе capture-перехватчик остался бы активен в других
	 *  вкладках и перехватывал клавиши. */
	let { active = true }: { active?: boolean } = $props();

	/** Пояснения свёрнуты: шесть абзацев в потоке занимали больше места, чем
	 *  все поля вместе. */
	let help = $state(false);

	let draft = $state<Settings | null>(null);
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

	$effect(() => {
		// Уход со вкладки сбрасывает запись хоткея. Остальное состояние (черновик,
		// раскрытый secret, подсказки) сохраняется — ради этого и держим вкладку живой.
		if (!active) recording = null;
	});

	const dirty = $derived(
		draft !== null &&
			app.settings !== null &&
			JSON.stringify($state.snapshot(draft)) !== JSON.stringify($state.snapshot(app.settings))
	);

	async function save() {
		if (!draft) return;
		saving = true;
		try {
			await app.saveSettings($state.snapshot(draft) as Settings);
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			saving = false;
		}
	}

	async function guard(action: () => Promise<unknown>) {
		try {
			await action();
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	async function pick(kind: 'config' | 'binary') {
		await guard(async () => {
			const path = await api.pickFile(kind);
			if (!path || !draft) return;
			if (kind === 'config') draft.singBox.configPath = path;
			else draft.singBox.binaryPath = path;
		});
	}

	async function newSecret() {
		await guard(async () => {
			if (!draft) return;
			draft.clashApi.secret = await api.generateSecret();
			secretVisible = true;
		});
	}

	async function copySecret() {
		if (!draft?.clashApi.secret) return;
		await guard(async () => {
			await navigator.clipboard.writeText(draft!.clashApi.secret);
			copied = true;
			setTimeout(() => (copied = false), 1500);
		});
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

	// Перехват на фазе capture: иначе Ctrl+1…7 успел бы переключить вкладку
	// раньше, чем запись увидит нажатие.
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

<svelte:window onkeydowncapture={onKeydown} />

<div class="page">
	{#if draft}
		<div class="toolbar">
			<span class="count">Настройки</span>
			<span class="spacer"></span>
			<button
				class="icon-btn"
				class:on={help}
				title="Показать пояснения к разделам"
				aria-label="Пояснения"
				onclick={() => (help = !help)}
			>
				<Icon name="info" size={13} />
			</button>
		</div>

		<!-- Все восемь разделов сразу, потоком по колонкам: в один свиток они
			 давали страницу примерно на 1700px при окне в 720, а в гриде под
			 короткой секцией оставалась пустота до конца ряда. -->
		<div class="masonry">
			<section class="section">
				<h3 class="section-title">Clash API</h3>
				<div class="form">
					<label>
						<span>Адрес</span>
						<input class="field" bind:value={draft.clashApi.url} placeholder="http://127.0.0.1:9797" />
					</label>
					<label>
						<span>Secret</span>
						<div class="combo">
							<input
								class="field"
								type={secretVisible ? 'text' : 'password'}
								bind:value={draft.clashApi.secret}
								placeholder="пусто — свой на каждый запуск"
							/>
							<button
								class="icon-btn"
								title={secretVisible ? 'Скрыть' : 'Показать'}
								aria-label={secretVisible ? 'Скрыть' : 'Показать'}
								onclick={() => (secretVisible = !secretVisible)}
							>
								<Icon name="search" size={12} />
							</button>
							{#if secretVisible}
								<button
									class="icon-btn"
									title={copied ? 'Скопировано' : 'Копировать'}
									aria-label="Копировать"
									disabled={!draft.clashApi.secret}
									onclick={copySecret}
								>
									<Icon name={copied ? 'check' : 'copy'} size={12} />
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
				</div>
				{#if help}
					<p class="hint">
						Изменение адреса, secret'а или уровня логов сразу переоткрывает подписки — перезапуск
						приложения не нужен. Пустой secret означает, что Vantage Box выдаст свой на каждый запуск
						sing-box.
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">sing-box</h3>
				<div class="form">
					<label>
						<span>Конфиг</span>
						<div class="combo">
							<input
								class="field"
								bind:value={draft.singBox.configPath}
								placeholder="путь к config.json"
							/>
							<button
								class="icon-btn"
								title="Выбрать файл"
								aria-label="Выбрать файл"
								onclick={() => pick('config')}
							>
								<Icon name="folder" size={12} />
							</button>
						</div>
					</label>
					<label>
						<span>Файл sing-box</span>
						<div class="combo">
							<input
								class="field"
								bind:value={draft.singBox.binaryPath}
								placeholder="пусто — под управлением Vantage Box"
							/>
							<button
								class="icon-btn"
								title="Выбрать файл"
								aria-label="Выбрать файл"
								onclick={() => pick('binary')}
							>
								<Icon name="folder" size={12} />
							</button>
						</div>
					</label>
					<label>
						<span>Обновление</span>
						<select bind:value={draft.singBox.updatePolicy}>
							<option value="off">не проверять</option>
							<option value="notify">уведомлять</option>
							<option value="auto">ставить автоматически</option>
						</select>
					</label>
				</div>
				{#if help}
					<p class="hint">
						Пустой путь к файлу sing-box означает бинарник под управлением приложения — версиями
						тогда можно рулить на вкладке «Сервис».
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">Интерфейс</h3>
				<div class="form">
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
						<input class="field" bind:value={draft.ui.latencyTestUrl} />
					</label>
					<label>
						<span>Таймаут теста, мс</span>
						<input
							class="num"
							type="number"
							min="100"
							max="60000"
							bind:value={draft.ui.latencyTestTimeout}
						/>
					</label>
				</div>
				{#if help}
					<p class="hint">
						По этому URL проверяется задержка узлов — и кнопкой в карточке группы, и
						автопереключением. Подходит любой адрес, отдающий короткий ответ.
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">Автопереключение</h3>
				<div class="form">
					<label>
						<span>Включено</span>
						<input type="checkbox" bind:checked={draft.fallback.enabled} />
					</label>
					<label>
						<span>Интервал проверки, с</span>
						<input
							class="num"
							type="number"
							min="5"
							max="3600"
							bind:value={draft.fallback.intervalSec}
						/>
					</label>
					<label>
						<span>Таймаут пинга, мс</span>
						<input
							class="num"
							type="number"
							min="100"
							max="60000"
							bind:value={draft.fallback.timeoutMs}
						/>
					</label>
					<label>
						<span>Предел задержки, мс</span>
						<input
							class="num"
							type="number"
							min="0"
							max="60000"
							title="0 — переключать только при полной недоступности"
							bind:value={draft.fallback.maxDelayMs}
						/>
					</label>
					<label>
						<span>Группы</span>
						<input
							class="field"
							value={draft.fallback.groups.join(', ')}
							placeholder="пусто — все selector-группы"
							oninput={(e) => {
								if (!draft) return;
								draft.fallback.groups = e.currentTarget.value
									.split(',')
									.map((g) => g.trim())
									.filter((g) => g !== '');
							}}
						/>
					</label>
				</div>
				{#if help}
					<p class="hint">
						Каждые <em>интервал</em> секунд активный узел selector-группы пингуется по URL
						latency-теста. При отказе или задержке выше предела группа переключается на узел с
						наименьшей задержкой. <code class="inline">urltest</code>-группы не затрагиваются — они
						рулят выбором сами. Предел 0 означает «переключать только когда узел совсем не ответил».
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">Трей и запуск</h3>
				<div class="form">
					<label>
						<span>Иконка в трее</span>
						<input type="checkbox" bind:checked={draft.tray.enabled} />
					</label>
					<label>
						<span>Закрывать в трей</span>
						<input type="checkbox" bind:checked={draft.tray.closeToTray} />
					</label>
					<label>
						<span>Стартовать свёрнутым</span>
						<input type="checkbox" bind:checked={draft.tray.startMinimized} />
					</label>
					<label>
						<span>Автозапуск при входе</span>
						<input type="checkbox" bind:checked={draft.autostart} />
					</label>
				</div>
				{#if help}
					<p class="hint">
						Иконка в трее и запуск свёрнутым появляются после перезапуска приложения — трей
						создаётся один раз при старте. Запуск свёрнутым без трея игнорируется: иначе окно стало
						бы нечем открыть.
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">Хоткеи</h3>
				<div class="form">
					{#each [{ id: 'proxyPopup', label: 'Попап прокси' }, { id: 'toggle', label: 'Вкл. / выкл.' }] as item (item.id)}
						{@const name = item.id as 'proxyPopup' | 'toggle'}
						<label>
							<span>{item.label}</span>
							<div class="combo">
								<input
									class="field"
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
								<button
									class="icon-btn"
									title="Очистить"
									aria-label="Очистить"
									disabled={draft.hotkeys[name] === ''}
									onclick={() => clearHotkey(name)}
								>
									<Icon name="close" size={12} />
								</button>
							</div>
						</label>
					{/each}
				</div>
				{#if app.hotkeyProblems.length > 0}
					<div class="banner">Не удалось занять: {app.hotkeyProblems.join(', ')}</div>
				{/if}
				{#if help}
					<p class="hint">
						«Записать» — и нажмите комбинацию целиком. Модификаторы:
						<code class="inline">Ctrl</code>, <code class="inline">Alt</code>,
						<code class="inline">Shift</code>, <code class="inline">Super</code> (клавиша Windows).
						Нужен хотя бы один: без модификатора хоткей отобрал бы клавишу у всей системы.
						<code class="inline">Esc</code> отменяет запись. Комбинацию можно и просто вписать
						руками, через <code class="inline">+</code>. Хоткеи работают глобально, даже когда окно
						закрыто.
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">Обновление приложения</h3>
				<div class="form">
					<label>
						<span>Проверка обновлений</span>
						<select bind:value={draft.guiUpdate.policy}>
							<option value="off">не проверять</option>
							<option value="notify">уведомлять</option>
							<option value="auto">ставить автоматически</option>
						</select>
					</label>
				</div>
				{#if help}
					<p class="hint">
						Обновления скачиваются с GitHub и проверяются подписью. «Уведомлять» показывает строку с
						кнопкой установки, «автоматически» ставит и перезапускает приложение без вопросов.
					</p>
				{/if}
			</section>

			<!-- Файл настроек — для тех, кто правит его руками; поэтому последним. -->
			<section class="section">
				<h3 class="section-title">Файл настроек</h3>
				<div class="form">
					<span class="lbl">Путь</span>
					<code class="path selectable ell" title={app.settingsPath}>{app.settingsPath}</code>
				</div>
				<div class="toolbar">
					<button onclick={() => guard(() => openPath(app.settingsPath))}>
						<Icon name="external" size={12} />
						Открыть
					</button>
					<button onclick={() => guard(() => revealItemInDir(app.settingsPath))}>
						<Icon name="folder" size={12} />
						Показать в папке
					</button>
				</div>
				{#if help}
					<p class="hint">
						Файл читается как JSONC — комментарии и висячие запятые допустимы. Правки
						подхватываются на лету, перезапуск не нужен.
					</p>
				{/if}
			</section>
		</div>

		<div class="sticky-footer">
			<button class="primary" onclick={save} disabled={!dirty || saving}>
				{saving ? 'Сохраняю…' : 'Сохранить'}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving}>Отменить</button>
			{#if dirty}<span class="hint">есть несохранённые изменения</span>{/if}
		</div>
	{:else}
		<p class="hint">Загружаю настройки…</p>
	{/if}
</div>

<style>
	/* Колонка, а не грид: плитка разделов лежит внутри .masonry, а тулбар и
	   полоса сохранения идут во всю ширину сами собой. min-height нужен, чтобы
	   `margin-top: auto` у полосы прижимал её к низу и на коротких формах. */
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
		min-height: 100%;
	}

	.count {
		font-weight: 600;
	}

	/* Поле с кнопками справа: кнопки по содержимому, поле забирает остаток. */
	.combo {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		min-width: 0;
		width: 100%;
	}

	.combo button:not(.icon-btn) {
		flex-shrink: 0;
	}

	.toolbar button {
		display: inline-flex;
		align-items: center;
		gap: var(--sp-2);
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
	}

	.hint {
		max-width: 62ch;
	}
</style>
