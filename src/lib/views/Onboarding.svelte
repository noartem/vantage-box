<script lang="ts">
	import { api, errorText } from '$lib/api';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
	import VersionsPanel from '$lib/components/VersionsPanel.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { Settings } from '$lib/types';

	let dismissed = $state(false);
	let busy = $state(false);
	// Onboarding errors stay in the dialog: the shared alert strip is beneath
	// the overlay and would not be visible from there.
	let error = $state<string | null>(null);

	const binaryOk = $derived(app.binaryInfo?.present === true);
	const configOk = $derived((app.settings?.singBox.configPath.trim() ?? '') !== '');

	/** Apply the changed settings: onboarding edits the same settings.json. */
	async function applySettings(mut: (s: Settings) => void) {
		if (!app.settings) return;
		busy = true;
		error = null;
		try {
			const next = structuredClone($state.snapshot(app.settings)) as Settings;
			mut(next);
			await app.saveSettings(next);
		} catch (e) {
			error = errorText(e);
		} finally {
			busy = false;
		}
	}

	async function guard(action: () => Promise<unknown>) {
		error = null;
		try {
			await action();
		} catch (e) {
			error = errorText(e);
		}
	}

	function pickConfig() {
		return guard(async () => {
			const path = await api.pickFile('config');
			if (path) await applySettings((s) => (s.singBox.configPath = path));
		});
	}

	function createMinimal() {
		return guard(async () => {
			const path = await api.createMinimalConfig();
			await applySettings((s) => (s.singBox.configPath = path));
		});
	}

	function pickBinary() {
		return guard(async () => {
			const path = await api.pickFile('binary');
			if (path) await applySettings((s) => (s.singBox.binaryPath = path));
		});
	}
</script>

{#if dismissed}
	<!-- So onboarding is not lost after "skip": a small return button. -->
	<button class="resume" onclick={() => (dismissed = false)} title={m.onboarding_resume_title()}>
		{m.onboarding_button()}
	</button>
{:else}
	<div class="overlay">
		<div class="dialog bounce">
			<header>
				<h2>{m.onboarding_welcome()}</h2>
				<span class="spacer"></span>
				<button class="ghost" onclick={() => (dismissed = true)}>{m.common_skip()}</button>
			</header>

			<p class="hint">
				{m.onboarding_hint()}
			</p>

			{#if error}
				<div class="banner warn">{error}</div>
			{/if}

			<section class="section step" data-done={binaryOk}>
				<div class="step-head">
					<h3 class="section-title">{m.onboarding_step_binary()}</h3>
					<span class="spacer"></span>
					{#if binaryOk}
						<span class="chip" data-tone="good">
							<Icon name="check" size={10} />
							{m.common_done()}
						</span>
					{/if}
				</div>

				<BinaryPanel />
				<VersionsPanel />

				<div class="toolbar">
					<button onclick={pickBinary} disabled={busy}>{m.onboarding_pick_binary()}</button>
				</div>
			</section>

			<section class="section step" data-done={configOk}>
				<div class="step-head">
					<h3 class="section-title">{m.onboarding_step_config()}</h3>
					<span class="spacer"></span>
					{#if configOk}
						<span class="chip" data-tone="good">
							<Icon name="check" size={10} />
							{m.common_done()}
						</span>
					{/if}
				</div>

				<p class="hint">
					{m.onboarding_config_hint()}
				</p>

				<div class="toolbar">
					<button onclick={pickConfig} disabled={busy}>{m.onboarding_pick_config()}</button>
					<button onclick={createMinimal} disabled={busy}>{m.onboarding_create_minimal()}</button>
				</div>
			</section>

			{#if binaryOk && configOk}
				<div class="banner ok">{m.onboarding_ready()}</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: flex-start;
		justify-content: center;
		padding: var(--sp-6);
		z-index: 100;
	}

	/* The dialog scrolls itself: together with the nested version catalog it
	   easily outgrew the window height. */
	.dialog {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius-card);
		max-width: 620px;
		width: 100%;
		max-height: 100%;
		overflow-y: auto;
		padding: var(--sp-5);
		display: grid;
		gap: var(--sp-4);
		align-content: start;
	}

	header {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	h2 {
		font-size: var(--fs-lg);
	}

	.ghost {
		background: transparent;
		border-color: transparent;
		color: var(--text-muted);
	}

	.step-head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	.step[data-done='true'] {
		border-color: var(--good);
	}

	.chip {
		gap: var(--sp-1);
	}

	.resume {
		position: fixed;
		right: var(--sp-5);
		bottom: calc(var(--h-status) + var(--sp-3));
		z-index: 100;
	}
</style>
