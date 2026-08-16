<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { app } from '$lib/state.svelte';
	import type { RestartOutcome, ServiceState } from '$lib/types';

	const SERVICE_LABELS: Record<ServiceState, string> = {
		notInstalled: 'не установлен',
		stopped: 'остановлен',
		startPending: 'запускается',
		running: 'работает',
		stopPending: 'останавливается',
		unknown: 'состояние неизвестно'
	};

	let busy = $state<string | null>(null);
	let help = $state(false);

	const run = $derived(app.run);
	const installed = $derived(run !== null && run.mode === 'service');
	const running = $derived(run?.running === true);
	/** Конфигу нужен TUN, а сервиса нет: запускать нечем, установка обязательна. */
	const serviceRequired = $derived(run !== null && !installed && run.tun);
	const configPath = $derived(app.settings?.singBox.configPath ?? '');
	const configMissing = $derived(configPath.trim() === '');

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			const result = await call();
			if (name === 'restart') report(result as RestartOutcome);
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
			await app.refreshRun();
		}
	}

	/** Итог перезапуска — событие, а не состояние: ему место в строке алертов,
	 *  а не в баннере, который потом некому убрать. */
	function report(outcome: RestartOutcome) {
		const skipped = outcome.skipped.length > 0 ? ` Пропущено: ${outcome.skipped.join('; ')}.` : '';
		if (!outcome.apiBack) {
			pushAlert('warn', `sing-box перезапущен, но Clash API не отозвался. Проверьте логи.${skipped}`);
			return;
		}
		const restored =
			outcome.restored.length > 0
				? `Восстановлен выбор: ${outcome.restored.join(', ')}.`
				: 'Выбор selector’ов менять не пришлось.';
		pushAlert('ok', `Перезапуск завершён. ${restored}${skipped}`);
	}
</script>

<div class="page">
	{#if !run}
		<p class="hint">Читаю состояние…</p>
	{:else}
		<section class="section">
			<div class="head">
				<h3 class="section-title">Состояние</h3>
				<span class="chip" data-tone={running ? 'good' : undefined}>
					{running ? 'работает' : 'остановлен'}
				</span>
				<span class="spacer"></span>
				<button
					class="icon-btn"
					class:on={help}
					title="Пояснения"
					aria-label="Пояснения"
					onclick={() => (help = !help)}
				>
					<Icon name="info" size={13} />
				</button>
			</div>

			<div class="form">
				<span class="lbl">Запуск</span>
				<span>
					{installed ? 'системная служба' : 'дочерний процесс'}
					{#if !installed && run.processPid !== null}
						<span class="muted mono">PID {run.processPid}</span>
					{/if}
				</span>

				<span class="lbl">TUN в конфиге</span>
				<span class:warnish={serviceRequired}>
					{run.tun ? 'есть — нужны права администратора' : 'нет'}
				</span>

				<span class="lbl">Конфиг</span>
				<code class="path ell selectable" title={configPath || 'не задан'}>
					{configPath || 'не задан'}
				</code>
			</div>

			{#if configMissing}
				<div class="banner warn">
					Не задан путь к config sing-box — укажите его в настройках, раздел «sing-box».
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
					{busy === 'restart' ? 'Перезапускаю…' : 'Мягкий перезапуск'}
				</button>
			</div>

			{#if help}
				<p class="hint">
					Мягкий перезапуск запоминает выбор в selector-группах и накатывает его обратно после
					старта — поверх того, что sing-box восстановит из
					<code class="inline">cache_file</code>.
				</p>
			{/if}
		</section>

		{#if run.service.supported}
			<section class="section">
				<div class="head">
					<h3 class="section-title">Системная служба</h3>
					<span class="chip" data-tone={installed && running ? 'good' : undefined}>
						{SERVICE_LABELS[run.service.state]}
					</span>
					<span class="spacer"></span>
				</div>

				<div class="form">
					<span class="lbl">Имя</span>
					<code class="path ell selectable" title={run.service.name}>{run.service.name}</code>
				</div>

				{#if run.service.detail}
					<div class="banner warn">{run.service.detail}</div>
				{/if}

				{#if serviceRequired}
					<div class="banner warn">
						В конфиге есть TUN-инбаунд: без установленной службы sing-box не запустится.
					</div>
				{/if}

				<div class="toolbar">
					<button
						class:primary={run.tun && !installed}
						disabled={busy !== null || configMissing}
						onclick={() => act('install', api.installService)}
					>
						{#if busy === 'install'}
							{installed ? 'Переустанавливаю…' : 'Устанавливаю…'}
						{:else}
							{installed ? 'Переустановить' : 'Установить службу'}
						{/if}
					</button>
					{#if installed}
						<button
							class="danger"
							disabled={busy !== null}
							onclick={() => act('uninstall', api.uninstallService)}
						>
							{busy === 'uninstall' ? 'Удаляю…' : 'Удалить'}
						</button>
					{/if}
				</div>

				{#if help}
					<p class="hint">
						{#if installed}
							Запуск и остановка идут через диспетчер служб и прав администратора не требуют: они
							выданы вашей учётной записи при установке. Переустановка нужна, если сменился путь к
							файлу sing-box или к конфигу. После удаления sing-box будет запускаться обычным
							процессом — это работает для любого конфига без TUN.
						{:else if run.tun}
							Служба обязательна: конфигу нужен TUN, а это права администратора. Установка
							запросит их один раз, дальше управление идёт без UAC.
						{:else}
							Этому конфигу служба не нужна — TUN в нём нет, и sing-box запускается обычным
							процессом от вашего имени. Служба пригодится, если позже добавите TUN или захотите,
							чтобы sing-box работал без запущенного Vantage Box.
						{/if}
					</p>
				{/if}
			</section>
		{:else}
			<p class="hint">{run.service.detail}</p>
		{/if}
	{/if}

	<BinaryPanel />
</div>

<style>
	.page {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
		align-items: start;
		gap: var(--sp-4);
		align-content: start;
	}

	/* Каталог версий — таблица: в колонке 340px он был бы нечитаем. */
	.page > :global(.versions) {
		grid-column: 1 / -1;
	}

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

	.warnish {
		color: var(--fair);
	}

	.hint {
		max-width: 62ch;
	}
</style>
