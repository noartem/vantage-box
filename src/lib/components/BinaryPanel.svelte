<script lang="ts">
	import { app } from '$lib/state.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import Icon from './Icon.svelte';
	import InfoButton from './InfoButton.svelte';

	// File details live in shared state: onboarding reads them too, to tell whether
	// a binary exists at all. There is intentionally no local copy here — otherwise
	// after a version change the two panels would show different things.
	const info = $derived(app.binaryInfo);
	const managed = $derived(info?.managed === true);

	let refreshing = $state(false);

	async function refresh() {
		refreshing = true;
		try {
			await app.refreshBinaryInfo();
		} finally {
			refreshing = false;
		}
	}
</script>

<section class="section">
	<div class="head">
		<h3 class="section-title">{m.binary_file_title()}</h3>
		<span class="spacer"></span>
		<button
			class="icon-btn"
			title={m.binary_refresh_title()}
			aria-label={m.common_refresh()}
			disabled={refreshing}
			onclick={refresh}
		>
			<Icon name="refresh" size={13} />
		</button>
		{#if info && !managed}
			<InfoButton label={() => m.common_explanations()}>
				<p>{m.binary_manual_hint()}</p>
			</InfoButton>
		{/if}
	</div>

	{#if info}
		<div class="form">
			<span class="lbl">{m.common_path()}</span>
			<code class="path ell selectable" title={info.path}>{info.path}</code>

			<span class="lbl">{m.binary_mode()}</span>
			<span>{info.managed ? m.binary_managed() : m.binary_manual()}</span>

			<span class="lbl">{m.common_version()}</span>
			<span class="mono">{info.version ?? (info.present ? m.service_version_undefined() : m.service_no_file())}</span>

			<span class="lbl">{m.binary_supported()}</span>
			<span class="mono muted">{info.supportedRange}</span>
		</div>

		{#if info.problem}
			<div class="banner">{info.problem}</div>
		{/if}

		{#if info.compatibility === 'tooOld'}
			<div class="banner warn">{m.binary_too_old()}</div>
		{:else if info.compatibility === 'tooNew'}
			<div class="banner warn">
				{m.binary_too_new()}
			</div>
		{/if}
	{:else}
		<p class="hint">{m.binary_reading()}</p>
	{/if}
</section>

<style>
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	.path {
		font-family: var(--mono);
		font-size: var(--fs-sm);
		max-width: 100%;
	}

	.hint {
		max-width: 62ch;
	}
</style>
