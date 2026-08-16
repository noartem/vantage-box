<script lang="ts">
	import { api } from '$lib/api';
	import { SERVICE_LABELS, runServiceAction } from '$lib/service-actions';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { onopen }: { onopen: () => void } = $props();

	let busy = $state<string | null>(null);

	const run = $derived(app.run);
	const installed = $derived(run !== null && run.mode === 'service');
	const running = $derived(run?.running === true);
	/** Конфигу нужен TUN, а сервиса нет: запускать нечем, установка обязательна. */
	const serviceRequired = $derived(run !== null && !installed && run.tun);
	const configMissing = $derived((app.settings?.singBox.configPath ?? '').trim() === '');

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}
</script>

<section class="section">
	<div class="head">
		<button class="title" title="Открыть вкладку «Сервис»" onclick={onopen}>
			<span class="section-title">Сервис</span>
			<Icon name="external" size={11} />
		</button>

		<span class="chip" data-tone={running ? 'good' : undefined}>
			{running ? 'работает' : 'остановлен'}
		</span>

		<span class="spacer"></span>
	</div>

	{#if !run}
		<p class="hint">Читаю состояние…</p>
	{:else}
		<div class="form">
			<span class="lbl">Запуск</span>
			<span>
				{installed ? 'системная служба' : 'дочерний процесс'}
				{#if !installed && run.processPid !== null}
					<span class="muted mono">PID {run.processPid}</span>
				{/if}
			</span>

			{#if run.service.supported}
				<span class="lbl">Служба</span>
				<span>{SERVICE_LABELS[run.service.state]}</span>
			{/if}

			<span class="lbl">TUN в конфиге</span>
			<span class:warnish={serviceRequired}>{run.tun ? 'есть' : 'нет'}</span>

			<span class="lbl">Версия</span>
			<span class="mono">
				{app.binaryInfo?.version ?? (app.binaryInfo?.present ? 'не определена' : 'нет файла')}
			</span>
		</div>

		{#if configMissing}
			<div class="banner warn">Не задан путь к config sing-box — укажите его в настройках.</div>
		{:else if serviceRequired}
			<div class="banner warn">
				В конфиге есть TUN-инбаунд: без установленной службы sing-box не запустится.
			</div>
		{/if}

		<div class="toolbar">
			<button
				class="primary"
				disabled={busy !== null || running || serviceRequired || configMissing}
				onclick={() => act('start', api.start)}
			>
				{busy === 'start' ? 'Запускаю…' : 'Запустить'}
			</button>
			<button disabled={busy !== null || !running} onclick={() => act('stop', api.stop)}>
				{busy === 'stop' ? 'Останавливаю…' : 'Остановить'}
			</button>
			<button
				disabled={busy !== null || !running || configMissing}
				onclick={() => act('restart', api.restart)}
			>
				{busy === 'restart' ? 'Перезапускаю…' : 'Перезапуск'}
			</button>
		</div>
	{/if}
</section>

<style>
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	/* Заголовок — кнопка перехода, но выглядеть должен подписью секции. */
	.title {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		height: auto;
		padding: 0;
		background: transparent;
		border: none;
		color: var(--text-muted);
	}

	.title:hover:not(:disabled) {
		border: none;
		color: var(--accent);
	}

	.warnish {
		color: var(--fair);
	}
</style>
