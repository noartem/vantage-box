<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import {
		LANGUAGE_OPTIONS,
		getLanguagePreference,
		setLanguagePreference,
		type LanguagePreference
	} from '$lib/i18n.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { Settings } from '$lib/types';

	/** Whether the settings tab is active. Tabs are not destroyed on switch, so
	 *  they do not know on their own whether they are visible. Needed to stop
	 *  hotkey recording when leaving — otherwise the capture listener would stay
	 *  active in other tabs and intercept keys. */
	let { active = true }: { active?: boolean } = $props();

	/** Explanations are collapsed: six paragraphs in flow took more space than
	 *  all the fields combined. */
	let help = $state(false);

	let draft = $state<Settings | null>(null);
	let saving = $state(false);
	/** The secret is hidden by default: it grants full control over sing-box. */
	let secretVisible = $state(false);
	let copied = $state(false);
	/** Which hotkey is currently being recorded from the keyboard. */
	let recording = $state<'proxyPopup' | 'toggle' | null>(null);
	/** The selected UI language: "system" or a specific locale. */
	let langPref = $state<LanguagePreference>(getLanguagePreference());

	$effect(() => {
		// settings.json is the source of truth. Edits to the file from outside
		// override the unsaved form: otherwise the UI would show what is not in the system.
		const current = app.settings;
		if (current) draft = structuredClone($state.snapshot(current)) as Settings;
	});

	$effect(() => {
		// Leaving the tab resets hotkey recording. The rest of the state (draft,
		// revealed secret, hints) is preserved — that is why we keep the tab alive.
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
	// Hotkey recording
	// -------------------------------------------------------------------------

	/** Keys whose accelerator name differs from `KeyboardEvent.code`. */
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

	/** The main key of the combination. `null` — only a modifier was pressed. */
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

	// Intercept on the capture phase: otherwise Ctrl+1…7 would switch the tab
	// before recording sees the press.
	function onKeydown(event: KeyboardEvent) {
		if (!recording || !draft) return;
		event.preventDefault();
		event.stopPropagation();

		// Esc exits recording without assigning itself: otherwise there would be no way out.
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

		// A global hotkey without a modifier would steal the key from the whole system.
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
			<span class="count">{m.settings_title()}</span>
			<span class="spacer"></span>
			<button
				class="icon-btn"
				class:on={help}
				title={m.settings_help_title()}
				aria-label={m.common_explanations()}
				onclick={() => (help = !help)}
			>
				<Icon name="info" size={13} />
			</button>
		</div>

		<!-- All nine sections at once, flowing through columns: in a single scroll
			 they gave a page of about 1700px in a 720px window, and in a grid a short
			 section left empty space to the end of the row. -->
		<div class="masonry">
			<section class="section">
				<h3 class="section-title">Clash API</h3>
				<div class="form">
					<label>
						<span>{m.settings_address()}</span>
						<input class="field" bind:value={draft.clashApi.url} placeholder="http://127.0.0.1:9797" />
					</label>
					<label>
						<span>Secret</span>
						<div class="combo">
							<input
								class="field"
								type={secretVisible ? 'text' : 'password'}
								bind:value={draft.clashApi.secret}
								placeholder={m.settings_secret_placeholder()}
							/>
							<button
								class="icon-btn"
								title={secretVisible ? m.common_hide() : m.common_show()}
								aria-label={secretVisible ? m.common_hide() : m.common_show()}
								onclick={() => (secretVisible = !secretVisible)}
							>
								<Icon name="search" size={12} />
							</button>
							{#if secretVisible}
								<button
									class="icon-btn"
									title={copied ? m.common_copied() : m.common_copy()}
									aria-label={m.common_copy()}
									disabled={!draft.clashApi.secret}
									onclick={copySecret}
								>
									<Icon name={copied ? 'check' : 'copy'} size={12} />
								</button>
							{/if}
							<button onclick={newSecret}>{m.settings_generate()}</button>
						</div>
					</label>
					<label>
						<span>{m.settings_log_level()}</span>
						<select bind:value={draft.clashApi.logLevel}>
							{#each ['trace', 'debug', 'info', 'warn', 'error'] as level (level)}
								<option value={level}>{level}</option>
							{/each}
						</select>
					</label>
				</div>
				{#if help}
					<p class="hint">
						{m.settings_help_clash_api()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">sing-box</h3>
				<div class="form">
					<label>
						<span>{m.common_config()}</span>
						<div class="combo">
							<input
								class="field"
								bind:value={draft.singBox.configPath}
								placeholder={m.settings_config_path_placeholder()}
							/>
							<button
								class="icon-btn"
								title={m.settings_pick_file_title()}
								aria-label={m.settings_pick_file_title()}
								onclick={() => pick('config')}
							>
								<Icon name="folder" size={12} />
							</button>
						</div>
					</label>
					<label>
						<span>{m.binary_file_title()}</span>
						<div class="combo">
							<input
								class="field"
								bind:value={draft.singBox.binaryPath}
								placeholder={m.settings_binary_path_placeholder()}
							/>
							<button
								class="icon-btn"
								title={m.settings_pick_file_title()}
								aria-label={m.settings_pick_file_title()}
								onclick={() => pick('binary')}
							>
								<Icon name="folder" size={12} />
							</button>
						</div>
					</label>
					<label>
						<span>{m.settings_update()}</span>
						<select bind:value={draft.singBox.updatePolicy}>
							<option value="off">{m.settings_update_off()}</option>
							<option value="notify">{m.settings_update_notify()}</option>
							<option value="auto">{m.settings_update_auto()}</option>
						</select>
					</label>
				</div>
				{#if help}
					<p class="hint">
						{m.settings_help_singbox()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">{m.settings_ui_section()}</h3>
				<div class="form">
					<label>
						<span>{m.settings_theme()}</span>
						<select bind:value={draft.ui.theme}>
							<option value="system">{m.settings_theme_system()}</option>
							<option value="light">{m.settings_theme_light()}</option>
							<option value="dark">{m.settings_theme_dark()}</option>
						</select>
					</label>
					<label>
						<span>{m.settings_latency_url()}</span>
						<input class="field" bind:value={draft.ui.latencyTestUrl} />
					</label>
					<label>
						<span>{m.settings_latency_timeout()}</span>
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
						{m.settings_help_ui()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">{m.settings_language()}</h3>
				<div class="form">
					<label>
						<span>{m.settings_language()}</span>
						<select
							bind:value={langPref}
							onchange={() => setLanguagePreference(langPref)}
						>
							{#each LANGUAGE_OPTIONS as opt (opt.value)}
								<option value={opt.value}>{opt.label}</option>
							{/each}
						</select>
					</label>
				</div>
				{#if help}
					<p class="hint">
						{m.settings_help_language()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">{m.settings_autoswitch()}</h3>
				<div class="form">
					<label>
						<span>{m.common_enabled()}</span>
						<input type="checkbox" bind:checked={draft.fallback.enabled} />
					</label>
					<label>
						<span>{m.settings_check_interval()}</span>
						<input
							class="num"
							type="number"
							min="5"
							max="3600"
							bind:value={draft.fallback.intervalSec}
						/>
					</label>
					<label>
						<span>{m.settings_ping_timeout()}</span>
						<input
							class="num"
							type="number"
							min="100"
							max="60000"
							bind:value={draft.fallback.timeoutMs}
						/>
					</label>
					<label>
						<span>{m.settings_max_delay()}</span>
						<input
							class="num"
							type="number"
							min="0"
							max="60000"
							title={m.settings_max_delay_hint()}
							bind:value={draft.fallback.maxDelayMs}
						/>
					</label>
					<label>
						<span>{m.common_groups()}</span>
						<input
							class="field"
							value={draft.fallback.groups.join(', ')}
							placeholder={m.settings_groups_placeholder()}
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
						{m.settings_help_autoswitch_pre()}
						<em>{m.settings_help_interval()}</em>
						{m.settings_help_autoswitch_mid()}
						<code class="inline">urltest</code>
						{m.settings_help_autoswitch_post()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">{m.settings_tray_section()}</h3>
				<div class="form">
					<label>
						<span>{m.settings_tray_icon()}</span>
						<input type="checkbox" bind:checked={draft.tray.enabled} />
					</label>
					<label>
						<span>{m.settings_close_to_tray()}</span>
						<input type="checkbox" bind:checked={draft.tray.closeToTray} />
					</label>
					<label>
						<span>{m.settings_start_minimized()}</span>
						<input type="checkbox" bind:checked={draft.tray.startMinimized} />
					</label>
					<label>
						<span>{m.settings_autostart()}</span>
						<input type="checkbox" bind:checked={draft.autostart} />
					</label>
				</div>
				{#if help}
					<p class="hint">
						{m.settings_help_tray()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">{m.settings_hotkeys()}</h3>
				<div class="form">
					{#each [{ id: 'proxyPopup', label: () => m.settings_hotkey_proxy_popup() }, { id: 'toggle', label: () => m.settings_hotkey_toggle() }] as item (item.id)}
						{@const name = item.id as 'proxyPopup' | 'toggle'}
						<label>
							<span>{item.label()}</span>
							<div class="combo">
								<input
									class="field"
									bind:value={draft.hotkeys[name]}
									placeholder={m.settings_hotkey_placeholder()}
									readonly={recording === name}
								/>
								<button
									class:primary={recording === name}
									onclick={() => (recording = recording === name ? null : name)}
								>
									{recording === name ? m.settings_recording() : m.settings_record()}
								</button>
								<button
									class="icon-btn"
									title={m.common_clear()}
									aria-label={m.common_clear()}
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
					<div class="banner">{m.settings_hotkey_failed()}: {app.hotkeyProblems.join(', ')}</div>
				{/if}
				{#if help}
					<p class="hint">
						{m.settings_help_hotkeys_a()}
						<code class="inline">Ctrl</code>, <code class="inline">Alt</code>,
						<code class="inline">Shift</code>, <code class="inline">Super</code>
						{m.settings_help_hotkeys_b()}
						<code class="inline">Esc</code>
						{m.settings_help_hotkeys_c()}
						<code class="inline">+</code>
						{m.settings_help_hotkeys_d()}
					</p>
				{/if}
			</section>

			<section class="section">
				<h3 class="section-title">{m.settings_app_update()}</h3>
				<div class="form">
					<label>
						<span>{m.settings_check_updates()}</span>
						<select bind:value={draft.guiUpdate.policy}>
							<option value="off">{m.settings_update_off()}</option>
							<option value="notify">{m.settings_update_notify()}</option>
							<option value="auto">{m.settings_update_auto()}</option>
						</select>
					</label>
				</div>
				{#if help}
					<p class="hint">
						{m.settings_help_app_update()}
					</p>
				{/if}
			</section>

			<!-- The settings file — for those who edit it by hand; hence last. -->
			<section class="section">
				<h3 class="section-title">{m.settings_file_section()}</h3>
				<div class="form">
					<span class="lbl">{m.common_path()}</span>
					<code class="path selectable ell" title={app.settingsPath}>{app.settingsPath}</code>
				</div>
				<div class="toolbar">
					<button onclick={() => guard(() => openPath(app.settingsPath))}>
						<Icon name="external" size={12} />
						{m.common_open()}
					</button>
					<button onclick={() => guard(() => revealItemInDir(app.settingsPath))}>
						<Icon name="folder" size={12} />
						{m.common_show_in_folder()}
					</button>
				</div>
				{#if help}
					<p class="hint">
						{m.settings_help_file()}
					</p>
				{/if}
			</section>
		</div>

		<div class="sticky-footer">
			<button class="primary" onclick={save} disabled={!dirty || saving}>
				{saving ? m.common_saving() : m.common_save()}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving}>{m.common_cancel()}</button>
			{#if dirty}<span class="hint">{m.common_unsaved_changes()}</span>{/if}
		</div>
	{:else}
		<p class="hint">{m.common_loading_settings()}</p>
	{/if}
</div>

<style>
	/* A column, not a grid: the section tile sits inside .masonry, while the
	   toolbar and save bar span the full width on their own. min-height is needed
	   so the bar's `margin-top: auto` pins it to the bottom even on short forms. */
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
		min-height: 100%;
	}

	.count {
		font-weight: 600;
	}

	/* Field with buttons on the right: buttons size to content, the field takes the rest. */
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
