<script lang="ts">
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	// Сведения о файле живут в общем состоянии: их же читает онбординг, чтобы
	// понять, есть ли вообще бинарник. Своей копии здесь нет намеренно —
	// иначе после смены версии две панели показывали бы разное.
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
		<h3 class="section-title">Файл sing-box</h3>
		<span class="spacer"></span>
		<button
			class="icon-btn"
			title="Перечитать сведения о файле"
			aria-label="Перечитать"
			disabled={refreshing}
			onclick={refresh}
		>
			<Icon name="refresh" size={13} />
		</button>
	</div>

	{#if info}
		<div class="form">
			<span class="lbl">Путь</span>
			<code class="path ell selectable" title={info.path}>{info.path}</code>

			<span class="lbl">Режим</span>
			<span>{info.managed ? 'под управлением Vantage Box' : 'задан вручную'}</span>

			<span class="lbl">Версия</span>
			<span class="mono">{info.version ?? (info.present ? 'не определена' : 'нет файла')}</span>

			<span class="lbl">Поддерживается</span>
			<span class="mono muted">{info.supportedRange}</span>
		</div>

		{#if info.problem}
			<div class="banner">{info.problem}</div>
		{/if}

		{#if info.compatibility === 'tooOld'}
			<div class="banner warn">Версия ниже поддерживаемого диапазона — обновите её.</div>
		{:else if info.compatibility === 'tooNew'}
			<div class="banner warn">
				Версия выше протестированного диапазона. Работать будет, но поведение может отличаться.
			</div>
		{/if}

		{#if !managed}
			<p class="hint">
				Путь задан вручную, поэтому Vantage Box этот файл не трогает — только сообщает о
				несовместимой версии. Очистите поле «Файл sing-box» в настройках, чтобы отдать версии
				приложению.
			</p>
		{/if}
	{:else}
		<p class="hint">Читаю сведения о файле…</p>
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
