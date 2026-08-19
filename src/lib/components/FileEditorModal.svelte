<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import type { JSONSchema7 } from 'json-schema';
	import CodeEditor from '$lib/components/CodeEditor.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import InfoButton from '$lib/components/InfoButton.svelte';
	import { errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { m } from '$lib/paraglide/messages.js';

	/** A reusable modal that shows a file in a CodeMirror editor. Read-only by
	 *  default; pass `onsave` to make it an editor with a Save button. The caller
	 *  supplies `load` (fetch the file) and, for editing, `onsave` (write it back). */
	let {
		open,
		onclose,
		title,
		hint,
		load,
		readOnly = false,
		onsave,
		version = null,
		schema = null
	}: {
		open: boolean;
		onclose: () => void;
		title: () => string;
		hint: () => string;
		load: () => Promise<{ path: string; content: string }>;
		readOnly?: boolean;
		onsave?: (content: string) => Promise<void>;
		/** sing-box version selects the linter schema; `null` — syntax-only. */
		version?: string | null;
		/** An explicit schema to lint/hover against — overrides the version lookup.
		 *  Used by the settings editor (settings.schema.json). */
		schema?: JSONSchema7 | null;
	} = $props();

	type LoadState =
		| { kind: 'loading' }
		| { kind: 'error'; message: string }
		| { kind: 'ready' };

	let loadState: LoadState = $state({ kind: 'loading' });
	let content = $state('');
	/** Baseline for the dirty check — the editor is clean when it matches the disk. */
	let saved = $state('');
	let path = $state('');
	let saving = $state(false);
	let copied = $state(false);

	const editable = $derived(!readOnly && !!onsave);
	const dirty = $derived(editable && content !== saved);

	// Fetch the file every time the modal is opened: it may have changed since
	// the last look (a save, an external edit, a restart).
	$effect(() => {
		if (!open) return;
		void reload();
	});

	async function reload() {
		loadState = { kind: 'loading' };
		try {
			const view = await load();
			content = view.content;
			saved = view.content;
			path = view.path;
			loadState = { kind: 'ready' };
		} catch (e) {
			loadState = { kind: 'error', message: errorText(e) };
		}
	}

	/** Re-reads without the loading flash — used after a save to show the
	 *  normalized on-disk file and clear the dirty flag. */
	async function sync() {
		try {
			const view = await load();
			content = view.content;
			saved = view.content;
			path = view.path;
			loadState = { kind: 'ready' };
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	async function save() {
		if (!onsave || !dirty || saving) return;
		saving = true;
		try {
			await onsave(content);
			await sync();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			saving = false;
		}
	}

	async function copy() {
		try {
			await navigator.clipboard.writeText(content);
			copied = true;
			setTimeout(() => (copied = false), 1500);
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	async function guard(action: () => Promise<unknown>) {
		try {
			await action();
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	// Escape closes — same instinct as every other dialog.
	function onKeydown(event: KeyboardEvent) {
		if (open && event.key === 'Escape') {
			event.preventDefault();
			onclose();
		}
	}
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
	<!-- Backdrop click closes; the dialog stops propagation so interacting with it does not. -->
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="overlay" onclick={onclose}>
		<div
			class="dialog"
			class:ready={loadState.kind === 'ready'}
			role="dialog"
			aria-modal="true"
			aria-label={title()}
			tabindex="-1"
			onclick={(e) => e.stopPropagation()}
		>
			<div class="head">
				<h3 class="title">{title()}</h3>
				{#if dirty}
					<span class="chip" data-tone="fair">{m.common_unsaved_changes()}</span>
				{/if}
				<span class="spacer"></span>
				<InfoButton label={() => m.common_explanations()}>
					<p>{hint()}</p>
				</InfoButton>
				{#if loadState.kind === 'ready'}
					<button
						class="icon-btn"
						title={copied ? m.common_copied() : m.common_copy()}
						aria-label={m.common_copy()}
						onclick={copy}
					>
						<Icon name="copy" size={13} />
					</button>
					<button
						class="icon-btn"
						title={m.config_open_external_title()}
						aria-label={m.config_open_external_title()}
						onclick={() => guard(() => openPath(path))}
					>
						<Icon name="external" size={13} />
					</button>
					<button
						class="icon-btn"
						title={m.common_show_in_folder()}
						aria-label={m.common_show_in_folder()}
						onclick={() => guard(() => revealItemInDir(path))}
					>
						<Icon name="folder" size={13} />
					</button>
				{/if}
				{#if editable}
					<button class="primary" onclick={save} disabled={!dirty || saving}>
						{saving ? m.common_saving() : m.common_save()}
					</button>
				{/if}
				<button class="icon-btn" title={m.common_close()} aria-label={m.common_close()} onclick={onclose}>
					<Icon name="close" size={13} />
				</button>
			</div>

			{#if loadState.kind === 'ready'}
				<code class="path ell selectable" title={path}>{path}</code>
			{/if}

			{#if loadState.kind === 'loading'}
				<p class="body placeholder">{m.common_loading()}</p>
			{:else if loadState.kind === 'error'}
				<div class="body banner warn">{loadState.message}</div>
			{:else}
				<div class="body editor-wrap">
					<CodeEditor value={content} {readOnly} onchange={(next) => (content = next)} onsave={save} {version} {schema} />
				</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 1001;
		display: flex;
		align-items: center;
		justify-content: center;
		background: color-mix(in srgb, var(--bg) 70%, transparent);
		padding: var(--sp-4);
	}

	.dialog {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		max-width: min(720px, 96vw);
		max-height: 90vh;
		width: 100%;
		padding: var(--sp-4);
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-card);
		box-shadow: 0 4px 24px rgba(0, 0, 0, 0.22);
	}

	/* A definite height only once the file is loaded: the editor's height:100%
	   chain (.editor → .cm-editor → .cm-scroller) needs a definite ancestor to
	   engage its own scroller — max-height alone is indefinite, so the editor
	   would grow to its full content and the dialog would clip instead of
	   scrolling. While loading or in error the dialog still sizes to content. */
	.dialog.ready {
		height: 90vh;
	}

	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}

	.title {
		margin: 0;
		font-size: var(--fs-md);
	}

	.spacer {
		flex: 1;
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
		max-width: 100%;
	}

	/* The editor stretches; the other body variants size to content. */
	.body {
		flex: 1;
		min-height: 0;
	}

	.placeholder {
		display: flex;
		align-items: center;
		color: var(--text-muted);
	}

	.banner {
		display: flex;
		align-items: center;
		padding: var(--sp-3);
		border-radius: var(--radius-ctl);
		font-size: var(--fs-sm);
	}

	.editor-wrap {
		position: relative;
		min-height: 240px;
		display: flex;
		min-width: 0;
	}

	.editor-wrap > :global(.editor) {
		flex: 1;
		min-width: 0;
	}
</style>