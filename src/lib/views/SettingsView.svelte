<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import InfoButton from '$lib/components/InfoButton.svelte';
	import SettingsFileModal from '$lib/components/SettingsFileModal.svelte';
	import {
		LANGUAGE_OPTIONS,
		getLanguagePreference,
		setLanguagePreference,
		type LanguagePreference
	} from '$lib/i18n.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { mainKey, modsFromEvent } from '$lib/hotkeys';
	import { app } from '$lib/state.svelte';
	import { settingsFileModal } from '$lib/settings-file.svelte';
	import type { Settings } from '$lib/types';

	/** The editable name of any hotkey binding. */
	type HotkeyName = keyof Settings['hotkeys'];

	/** Global hotkeys — registered with the OS, work even when the window is closed. */
	const GLOBAL_HOTKEYS: { id: HotkeyName; label: () => string }[] = [
		{ id: 'proxyPopup', label: () => m.settings_hotkey_proxy_popup() },
		{ id: 'toggle', label: () => m.settings_hotkey_toggle() },
		{ id: 'showMain', label: () => m.settings_hotkey_show_main() },
		{ id: 'restart', label: () => m.settings_hotkey_restart() }
	];

	/** In-app shortcuts — matched against keydown events while the window is focused. */
	const INAPP_HOTKEYS: { id: HotkeyName; label: () => string }[] = [
		{ id: 'goToSettings', label: () => m.settings_hotkey_go_to_settings() },
		{ id: 'nextTab', label: () => m.settings_hotkey_next_tab() },
		{ id: 'prevTab', label: () => m.settings_hotkey_prev_tab() },
		{ id: 'tabIndex', label: () => m.settings_hotkey_tab_index() },
		{ id: 'closeWindow', label: () => m.settings_hotkey_close_window() }
	];

	/** Whether the settings tab is active. Tabs are not destroyed on switch, so
	 *  they do not know on their own whether they are visible. Needed to stop
	 *  hotkey recording when leaving — otherwise the capture listener would stay
	 *  active in other tabs and intercept keys. */
	let { active = true }: { active?: boolean } = $props();

	let draft = $state<Settings | null>(null);
	let saving = $state(false);
	/** The secret is hidden by default: it grants full control over sing-box. */
	let secretVisible = $state(false);
	let copied = $state(false);
	/** Which hotkey is currently being recorded from the keyboard. */
	let recording = $state<HotkeyName | null>(null);
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

	/** Click the settings file path to copy it. */
	let pathCopied = $state(false);
	async function copyPath() {
		await guard(async () => {
			await navigator.clipboard.writeText(app.settingsPath);
			pathCopied = true;
			setTimeout(() => (pathCopied = false), 1500);
		});
	}

	// -------------------------------------------------------------------------
	// Hotkey recording
	// -------------------------------------------------------------------------

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

		const mods = modsFromEvent(event);
		// A hotkey without a modifier would steal the key from the whole system.
		if (mods.length === 0) return;

		if (recording === 'tabIndex') {
			// Tab-by-index binds a modifier prefix; digits 1–9 are appended at
			// runtime. The pressed key only confirms the modifiers — we drop it.
			draft.hotkeys.tabIndex = mods.join('+');
		} else {
			draft.hotkeys[recording] = [...mods, key].join('+');
		}
		recording = null;
	}

	function clearHotkey(name: HotkeyName) {
		if (!draft) return;
		draft.hotkeys[name] = '';
		recording = null;
	}
</script>

<svelte:window onkeydowncapture={onKeydown} />

<div class="page">
	{#if draft}
		<!-- All nine sections at once, flowing through columns: in a single scroll
			 they gave a page of about 1700px in a 720px window, and in a grid a short
			 section left empty space to the end of the row. Each section's title
			 carries its own "?" with the explanation. -->
		<div class="masonry">
			<section class="section">
				<div class="head">
					<h3 class="section-title">Clash API</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_clash_api()}</p>
					</InfoButton>
				</div>
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
			</section>

			<section class="section">
				<div class="head">
					<h3 class="section-title">sing-box</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_singbox()}</p>
					</InfoButton>
				</div>
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
			</section>

			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_ui_section()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_ui()}</p>
					</InfoButton>
				</div>
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
			</section>

			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_language()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_language()}</p>
					</InfoButton>
				</div>
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
			</section>

			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_autoswitch()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>
							{m.settings_help_autoswitch_pre()}
							<em>{m.settings_help_interval()}</em>
							{m.settings_help_autoswitch_mid()}
							<code class="inline">urltest</code>
							{m.settings_help_autoswitch_post()}
						</p>
					</InfoButton>
				</div>
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
			</section>

			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_tray_section()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_tray()}</p>
					</InfoButton>
				</div>
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
			</section>

			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_hotkeys()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>
							{m.settings_help_hotkeys_a()}
							<code class="inline">Ctrl</code>, <code class="inline">Alt</code>,
							<code class="inline">Shift</code>, <code class="inline">Super</code>
							{m.settings_help_hotkeys_b()}
							<code class="inline">Esc</code>
							{m.settings_help_hotkeys_c()}
							<code class="inline">+</code>
							{m.settings_help_hotkeys_d()}
						</p>
					</InfoButton>
				</div>
				<div class="form">
					<p class="group-label">{m.settings_hotkeys_global()}</p>
					{#each GLOBAL_HOTKEYS as item (item.id)}
						{@render hotkeyRow(item)}
					{/each}
					<p class="group-label">{m.settings_hotkeys_inapp()}</p>
					{#each INAPP_HOTKEYS as item (item.id)}
						{@render hotkeyRow(item)}
					{/each}
				</div>
				{#if app.hotkeyProblems.length > 0}
					<div class="banner">{m.settings_hotkey_failed()}: {app.hotkeyProblems.join(', ')}</div>
				{/if}
			</section>

			{#snippet hotkeyRow(item: { id: HotkeyName; label: () => string })}
				<label>
					<span>{item.label()}</span>
					<div class="combo">
						<input
							class="field"
							bind:value={draft!.hotkeys[item.id]}
							placeholder={m.settings_hotkey_placeholder()}
							readonly={recording === item.id}
						/>
						{#if item.id === 'tabIndex'}
							<span class="affix" title={m.settings_hotkey_tab_index_hint()}>1…9</span>
						{/if}
						<button
							class:primary={recording === item.id}
							onclick={() => (recording = recording === item.id ? null : item.id)}
						>
							{recording === item.id ? m.settings_recording() : m.settings_record()}
						</button>
						<button
							class="icon-btn"
							title={m.common_clear()}
							aria-label={m.common_clear()}
							disabled={draft!.hotkeys[item.id] === ''}
							onclick={() => clearHotkey(item.id)}
						>
							<Icon name="close" size={12} />
						</button>
					</div>
				</label>
			{/snippet}

			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_app_update()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_app_update()}</p>
					</InfoButton>
				</div>
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
			</section>

			<!-- The settings file — for those who edit it by hand; hence last. -->
			<section class="section">
				<div class="head">
					<h3 class="section-title">{m.settings_file_section()}</h3>
					<span class="spacer"></span>
					<InfoButton label={() => m.common_explanations()}>
						<p>{m.settings_help_file()}</p>
					</InfoButton>
				</div>
				<div class="form baseline-row">
					<span class="lbl">{m.common_path()}</span>
					<!-- Click the path to copy it; the icon flips to a check briefly. -->
					<button
						class="copy-path"
						title={pathCopied ? m.common_copied() : app.settingsPath}
						aria-label={m.common_copy()}
						onclick={copyPath}
					>
						<span class="path ell">{app.settingsPath}</span>
						<Icon name={pathCopied ? 'check' : 'copy'} size={12} />
					</button>
				</div>
				<div class="toolbar">
					<button class="primary" onclick={() => settingsFileModal.show()}>
						<Icon name="edit" size={12} />
						{m.settings_file_edit()}
					</button>
					<button onclick={() => guard(() => openPath(app.settingsPath))}>
						<Icon name="external" size={12} />
						{m.common_open()}
					</button>
					<button onclick={() => guard(() => revealItemInDir(app.settingsPath))}>
						<Icon name="folder" size={12} />
						{m.common_show_in_folder()}
					</button>
				</div>
			</section>
		</div>

		<div class="sticky-footer">
			<button class="primary" onclick={save} disabled={!dirty || saving}>
				{saving ? m.common_saving() : m.common_save()}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving}>{m.common_cancel()}</button>
			{#if dirty}<span class="hint">{m.common_unsaved_changes()}</span>{/if}
		</div>

		<!-- In-app editor for settings.json. Store-driven, so it can be opened from
		     the file section above without prop drilling. -->
		<SettingsFileModal />
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

	/* Section title row: the label and its "?" explanation button. */
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
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

	/* The settings file path as a click-to-copy button: mono, ellipsized, accent
	   on hover — reads like the code it replaces. Resets the global button chrome
	   (background/border/22px height) so it sits on the value line. */
	.copy-path {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		width: 100%;
		font-family: var(--mono);
		font-size: var(--fs-sm);
		text-align: left;
		height: auto;
		min-height: 0;
		padding: 0;
		background: transparent;
		border: none;
		color: var(--text);
		cursor: pointer;
	}

	.copy-path:hover {
		border: none;
		color: var(--accent);
	}

	.copy-path .path {
		flex: 1;
		min-width: 0;
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
	}

	/* The path row: align the "Path" label with the mono path by baseline, not
	   box center — the mono font sits higher than the sans label, so center
	   alignment made the path read above the label. */
	.baseline-row {
		align-items: baseline;
	}

	.hint {
		max-width: 62ch;
	}

	/* Subheader inside the hotkeys section: separates global from in-app bindings.
	   Must span both grid columns — otherwise it shifts every row after it by one
	   column and the labels/fields end up on opposite sides between groups. */
	.group-label {
		grid-column: 1 / -1;
		margin: 0;
		font-size: var(--fs-sm);
		font-weight: 600;
		color: var(--text-muted);
	}

	.group-label:not(:first-child) {
		margin-top: var(--sp-2);
	}

	/* The "1…9" suffix on the tab-by-index row: digits are appended to the stored
	   modifier prefix at runtime, so the field shows the prefix only. */
	.affix {
		flex-shrink: 0;
		font-family: var(--mono);
		font-size: var(--fs-sm);
		color: var(--text-muted);
	}
</style>
