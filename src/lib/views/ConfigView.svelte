<script lang="ts">
	import { untrack } from 'svelte';
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import CodeEditor, { type EditorDiagnostic } from '$lib/components/CodeEditor.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { CheckResult } from '$lib/types';

	let content = $state('');
	let saved = $state('');
	let loaded = $state(false);
	let busy = $state<string | null>(null);
	let check = $state<CheckResult | null>(null);
	/** Shown after a successful save: sing-box reads the config only at startup. */
	let needsRestart = $state(false);
	let showOutput = $state(false);

	/** Editor diagnostics (schema + JSON5 linter) — for the chip and the error list. */
	let diags = $state<EditorDiagnostic[]>([]);
	let showErrors = $state(false);
	/** Reference to the editor, to jump to a line from the error list. */
	let editor = $state<{ jumpTo: (from: number, to: number) => void } | null>(null);

	const path = $derived((app.settings?.singBox.configPath ?? '').trim());
	const dirty = $derived(content !== saved);

	/** Whether there are real value errors — those not filtered out as version noise. */
	const errorCount = $derived(diags.filter((d) => d.severity === 'error').length);

	function goto(diag: EditorDiagnostic) {
		editor?.jumpTo(diag.from, diag.to);
		showErrors = false;
	}

	type Notice = {
		tone: 'error' | 'warn' | 'ok';
		text: string;
		action?: { label: string; run: () => void };
	};

	/** Previously up to four banners could stack above the editor at once and
	 *  eat a third of its height. We show the most urgent one in a single line. */
	const notice = $derived.by<Notice | null>(() => {
		if (check && !check.ok) {
			return { tone: 'error', text: m.config_check_failed({ output: firstLine(check.output) }) };
		}
		if (app.configChangedExternally) {
			return {
				tone: 'warn',
				text: dirty ? m.config_changed_externally_dirty() : m.config_changed_externally(),
				action: { label: m.config_reload(), run: load }
			};
		}
		if (needsRestart) {
			return {
				tone: 'warn',
				text: m.config_saved_restart()
			};
		}
		if (check?.ok) {
			return {
				tone: 'ok',
				text: check.available ? m.config_check_ok() : m.config_json_ok({ output: check.output })
			};
		}
		return null;
	});

	function firstLine(text: string): string {
		return text.split('\n')[0] ?? text;
	}

	async function load() {
		busy = 'load';
		try {
			const text = await api.readSingboxConfig();
			content = text;
			saved = text;
			check = null;
			showOutput = false;
			app.configChangedExternally = null;
			loaded = true;
		} catch (e) {
			pushAlert('error', errorText(e));
			loaded = false;
		} finally {
			busy = null;
		}
	}

	async function validate(): Promise<CheckResult | null> {
		busy = 'check';
		try {
			check = await api.checkSingboxConfig(content);
			return check;
		} catch (e) {
			pushAlert('error', errorText(e));
			return null;
		} finally {
			busy = null;
		}
	}

	async function save() {
		// Validate before writing: a broken config.json breaks the next sing-box
		// startup, and rolling back afterward would have to be done by hand.
		const result = await validate();
		if (!result || !result.ok) return;

		busy = 'save';
		try {
			await api.writeSingboxConfig(content);
			saved = content;
			needsRestart = true;
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
		}
	}

	async function guard(action: () => Promise<unknown>) {
		try {
			await action();
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	$effect(() => {
		// Re-read when the path in settings changes.
		path;
		if (path) load();
	});

	$effect(() => {
		// The file changed outside the app. Re-read it to sync `saved` with the
		// actual on-disk content — then the "not saved" chip reflects the
		// discrepancy more accurately. If the disk content equals the editor
		// content, there is no discrepancy: clear the flag, and the chip and banner go away.
		// `content` is read via untrack — otherwise the effect would depend on every keystroke.
		const ext = app.configChangedExternally;
		if (!ext) return;
		void (async () => {
			try {
				const text = await api.readSingboxConfig();
				saved = text;
				if (text === untrack(() => content)) app.configChangedExternally = null;
			} catch {
				// Could not read — leave the notification as is.
			}
		})();
	});
</script>

<div class="page">
	{#if !path}
		<p class="hint">
			{m.config_path_missing_pre()}
			<code class="inline">config.json</code>
			{m.config_path_missing_post()}
		</p>
	{:else}
		<!-- The bar stays on a single line: previously a long path with word-break
			 wrapped it onto two and shifted the editor down. -->
		<div class="toolbar">
			<code class="path ell selectable" title={path}>{path}</code>

			{#if dirty}<span class="chip" data-tone="fair">{m.config_not_saved()}</span>{/if}

			{#if errorCount > 0}
				<!-- Trigger chip: opens the editor error list, clicking a row jumps to it. -->
				<button
					class="chip err-chip"
					data-tone="poor"
					data-open={showErrors}
					aria-expanded={showErrors}
					onclick={() => (showErrors = !showErrors)}
					title={m.config_editor_errors_title()}
				>
					<Icon name="alert" size={13} />
					{m.config_errors_label()}: {errorCount}
				</button>
			{/if}

			<span class="spacer"></span>

			<button
				class="icon-btn"
				title={m.config_check_title()}
				aria-label={m.config_check_label()}
				disabled={busy !== null || !loaded}
				onclick={validate}
			>
				<Icon name="check" size={13} />
			</button>
			<button
				class="icon-btn"
				title={m.config_reload_disk_title()}
				aria-label={m.config_reload()}
				disabled={busy !== null}
				onclick={load}
			>
				<Icon name="refresh" size={13} />
			</button>
			<button
				class="icon-btn"
				title={m.config_open_external_title()}
				aria-label={m.config_open_external_title()}
				disabled={busy !== null}
				onclick={() => guard(() => openPath(path))}
			>
				<Icon name="external" size={13} />
			</button>
			<button
				class="icon-btn"
				title={m.common_show_in_folder()}
				aria-label={m.common_show_in_folder()}
				disabled={busy !== null}
				onclick={() => guard(() => revealItemInDir(path))}
			>
				<Icon name="folder" size={13} />
			</button>
			<button
				class="icon-btn"
				title={m.config_docs_title()}
				aria-label={m.config_docs_title()}
				disabled={busy !== null}
				onclick={() => guard(() => openPath('https://sing-box.sagernet.org/configuration/'))}
			>
				<Icon name="book" size={13} />
			</button>
			<button class="primary" onclick={save} disabled={busy !== null || !loaded || !dirty}>
				{busy === 'save' ? m.common_saving() : busy === 'check' ? m.config_checking() : m.common_save()}
			</button>
		</div>

		{#if notice}
			<div class="notice" data-tone={notice.tone}>
				<Icon name={notice.tone === 'ok' ? 'info' : 'alert'} size={12} />
				<span class="ell selectable" title={notice.text}>{notice.text}</span>
				{#if notice.action}
					<button class="act" onclick={notice.action.run}>{notice.action.label}</button>
				{/if}
				{#if check && !check.ok}
					<button class="act" onclick={() => (showOutput = !showOutput)}>
						{showOutput ? m.common_collapse() : m.common_details()}
					</button>
				{/if}
			</div>
		{/if}

		{#if showOutput && check && !check.ok}
			<pre class="output selectable bounce">{check.output}</pre>
		{/if}

		{#if loaded}
			<div class="editor-wrap">
				<CodeEditor
					bind:this={editor}
					value={content}
					version={app.status.version}
					onchange={(next) => {
						content = next;
						needsRestart = false;
						check = null;
						showOutput = false;
					}}
					ondiagnostics={(next) => {
						diags = next;
						// If errors collapsed — close the popup so it does not hang empty.
						if (next.length === 0) showErrors = false;
					}}
					onsave={save}
				/>

				{#if showErrors && diags.length > 0}
					<div class="err-popup card">
						<div class="err-head">
							<span>{m.config_editor_errors_title()}: {diags.length}</span>
							<button
								class="act"
								aria-label={m.config_close_list()}
								onclick={() => (showErrors = false)}
							>
								✕
							</button>
						</div>
						<div class="err-list">
							{#each diags as diag, i (i)}
								<button class="err-row" onclick={() => goto(diag)}>
									<span class="err-loc">{diag.line}:{diag.col}</span>
									<span class="err-msg selectable">{diag.message}</span>
								</button>
							{/each}
						</div>
					</div>
				{/if}
			</div>
		{/if}
	{/if}
</div>

<style>
	/* Flex, not grid: the result row comes and goes, and pinning the editor to
	   a specific grid row would be fragile. */
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		height: 100%;
		min-height: 0;
	}

	.toolbar {
		flex-wrap: nowrap;
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
		color: var(--text-muted);
		min-width: 0;
	}

	.notice {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-alert);
		padding: 0 var(--sp-3);
		border-radius: var(--radius-ctl);
		font-size: var(--fs-sm);
		color: var(--poor);
		background: color-mix(in srgb, var(--poor) 12%, transparent);
		flex-shrink: 0;
	}

	.notice[data-tone='warn'] {
		color: var(--fair);
		background: color-mix(in srgb, var(--fair) 12%, transparent);
	}

	.notice[data-tone='ok'] {
		color: var(--good);
		background: color-mix(in srgb, var(--good) 12%, transparent);
	}

	.notice .ell {
		flex: 1;
	}

	.act {
		height: 18px;
		padding: 0 var(--sp-3);
		font-size: var(--fs-xs);
		background: transparent;
		border-color: currentcolor;
		color: inherit;
	}

	.output {
		margin: 0;
		max-height: 96px;
		overflow: auto;
		padding: var(--sp-3);
		border: 1px solid var(--border);
		border-radius: var(--radius-ctl);
		background: var(--surface);
		font-family: var(--mono);
		font-size: var(--fs-sm);
		white-space: pre-wrap;
		word-break: break-word;
		flex-shrink: 0;
	}

	/* Only the editor stretches; everything else sizes to content. */
	.editor-wrap {
		position: relative;
		flex: 1;
		min-height: 180px;
		display: flex;
		min-width: 0;
	}

	.editor-wrap > :global(.editor) {
		flex: 1;
		min-width: 0;
	}

	/* Trigger chip for the error list: the button drops its border, the rest comes from .chip. */
	.err-chip {
		gap: var(--sp-1);
		border: none;
		cursor: pointer;
		line-height: 1;
	}

	.err-chip[data-open='true'] {
		outline: 1px solid currentcolor;
	}

	/* Error-list popup: sits in the top-right corner of the editor. */
	.err-popup {
		position: absolute;
		top: var(--sp-2);
		right: var(--sp-2);
		z-index: 5;
		width: min(440px, 86%);
		max-height: 64%;
		display: flex;
		flex-direction: column;
		padding: var(--sp-2);
		gap: var(--sp-1);
		box-shadow: var(--shadow, 0 4px 16px rgba(0, 0, 0, 0.25));
	}

	.err-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: var(--fs-sm);
		color: var(--text-muted);
		padding: 0 var(--sp-1);
	}

	.err-list {
		overflow: auto;
		display: flex;
		flex-direction: column;
		gap: 1px;
	}

	.err-row {
		display: flex;
		gap: var(--sp-2);
		align-items: baseline;
		text-align: left;
		padding: var(--sp-1) var(--sp-2);
		border: none;
		border-radius: var(--radius-ctl);
		background: transparent;
		color: var(--text);
		cursor: pointer;
		font-size: var(--fs-sm);
		line-height: 1.35;
	}

	.err-row:hover {
		background: var(--surface-alt);
	}

	.err-loc {
		flex-shrink: 0;
		font-family: var(--mono);
		font-size: var(--fs-xs);
		color: var(--poor);
		min-width: 3.5em;
	}

	.err-msg {
		min-width: 0;
		word-break: break-word;
	}
</style>
