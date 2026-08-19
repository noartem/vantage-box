<script lang="ts">
	import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
	import CodeEditor from '$lib/components/CodeEditor.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { runtimeConfigModal } from '$lib/runtime-config.svelte';
	import type { RuntimeConfigView } from '$lib/types';

	type LoadState =
		| { kind: 'loading' }
		| { kind: 'error'; message: string }
		| { kind: 'ready'; view: RuntimeConfigView };

	let loadState: LoadState = $state({ kind: 'loading' });
	let copied = $state(false);

	const open = $derived(runtimeConfigModal.open);

	// Fetch the runtime config every time the viewer is opened: it may have
	// changed since the last look (a restart writes a fresh secret).
	$effect(() => {
		if (!open) return;
		loadState = { kind: 'loading' };
		void (async () => {
			try {
				const view = await api.readRuntimeConfig();
				loadState = { kind: 'ready', view };
			} catch (e) {
				loadState = { kind: 'error', message: errorText(e) };
			}
		})();
	});

	function close() {
		runtimeConfigModal.hide();
	}

	// Escape closes the viewer — same instinct as every other dialog.
	function onKeydown(event: KeyboardEvent) {
		if (open && event.key === 'Escape') {
			event.preventDefault();
			close();
		}
	}

	async function copy() {
		if (loadState.kind !== 'ready') return;
		try {
			await navigator.clipboard.writeText(loadState.view.content);
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
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
	<!-- Backdrop click closes; the dialog stops propagation so interacting with it does not. -->
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="overlay" onclick={close}>
		<div
			class="dialog"
			role="dialog"
			aria-modal="true"
			aria-label={m.runtime_config_title()}
			tabindex="-1"
			onclick={(e) => e.stopPropagation()}
		>
			<div class="head">
				<h3 class="title">{m.runtime_config_title()}</h3>
				<span class="spacer"></span>
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
						onclick={() => guard(() => openPath(loadState.kind === 'ready' ? loadState.view.path : ''))}
					>
						<Icon name="external" size={13} />
					</button>
					<button
						class="icon-btn"
						title={m.common_show_in_folder()}
						aria-label={m.common_show_in_folder()}
						onclick={() =>
							guard(() => revealItemInDir(loadState.kind === 'ready' ? loadState.view.path : ''))}
					>
						<Icon name="folder" size={13} />
					</button>
				{/if}
				<button class="icon-btn" title={m.common_close()} aria-label={m.common_close()} onclick={close}>
					<Icon name="close" size={13} />
				</button>
			</div>

			{#if loadState.kind === 'ready'}
				<code class="path ell selectable" title={loadState.view.path}>{loadState.view.path}</code>
			{/if}

			<p class="hint">{m.runtime_config_hint()}</p>

			{#if loadState.kind === 'loading'}
				<p class="body placeholder">{m.runtime_config_loading()}</p>
			{:else if loadState.kind === 'error'}
				<div class="body banner warn">{loadState.message}</div>
			{:else}
				<div class="body editor-wrap">
					<CodeEditor value={loadState.view.content} readOnly />
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

	.hint {
		margin: 0;
		max-width: 70ch;
		font-size: var(--fs-sm);
		color: var(--text-muted);
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