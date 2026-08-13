<script lang="ts">
	import { api, errorText } from '$lib/api';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
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
	let error = $state<string | null>(null);
	/** Итог последнего мягкого перезапуска. */
	let restart_ = $state<RestartOutcome | null>(null);

	const run = $derived(app.run);
	const installed = $derived(run !== null && run.mode === 'service');
	const running = $derived(run?.running === true);
	/** Конфигу нужен TUN, а сервиса нет: запускать нечем, установка обязательна. */
	const serviceRequired = $derived(run !== null && !installed && run.tun);
	const configMissing = $derived((app.settings?.singBox.configPath ?? '').trim() === '');

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		error = null;
		if (name !== 'restart') restart_ = null;
		try {
			const result = await call();
			if (name === 'restart') restart_ = result as RestartOutcome;
		} catch (e) {
			error = errorText(e);
		} finally {
			busy = null;
			await app.refreshRun();
		}
	}
</script>

<div class="page">
	{#if error}
		<div class="banner">{error}</div>
	{/if}

	<section class="card">
		<header>
			<div class="title">
				<h3>sing-box</h3>
				<span class="state" data-running={running}>
					{running ? 'работает' : 'остановлен'}
				</span>
			</div>
			{#if run}
				<span class="muted mode">
					{#if installed}
						сервис · {SERVICE_LABELS[run.service.state]}
					{:else if run.processPid !== null}
						процесс · PID {run.processPid}
					{:else}
						процесс
					{/if}
				</span>
			{/if}
		</header>

		{#if run && !run.service.supported}
			<p class="muted">{run.service.detail}</p>
		{:else if run}
			{#if run.service.detail}
				<div class="banner warn">{run.service.detail}</div>
			{/if}

			{#if configMissing}
				<div class="banner warn">
					Не задан путь к config sing-box — укажите его на вкладке «Настройки».
				</div>
			{:else if run.configProblem}
				<div class="banner warn">Конфиг не прочитан: {run.configProblem}</div>
			{/if}

			{#if serviceRequired}
				<div class="banner warn">
					В конфиге есть TUN-инбаунд. TUN поднимает сетевой адаптер, а это права
					администратора — без установленного сервиса sing-box не запустится.
				</div>
			{/if}

			<div class="actions">
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

			{#if restart_}
				<div class="banner" class:ok={restart_.apiBack} class:warn={!restart_.apiBack}>
					{#if restart_.apiBack}
						<strong>Перезапуск завершён.</strong>
						{#if restart_.restored.length > 0}
							Восстановлен выбор: {restart_.restored.join(', ')}.
						{:else}
							Выбор selector'ов менять не пришлось — sing-box поднял его сам.
						{/if}
					{:else}
						<strong>sing-box перезапущен, но Clash API не отозвался.</strong>
						Проверьте логи sing-box.
					{/if}
					{#if restart_.skipped.length > 0}
						<div class="muted skipped">Пропущено: {restart_.skipped.join('; ')}</div>
					{/if}
				</div>
			{/if}

			<p class="muted hint">
				Мягкий перезапуск запоминает выбор в selector-группах и накатывает его обратно после
				старта — поверх того, что sing-box восстановит из <code>cache_file</code>.
			</p>
		{:else}
			<p class="muted">Читаю состояние…</p>
		{/if}
	</section>

	{#if run && run.service.supported}
		<section class="card">
			<header>
				<div class="title">
					<h3>Системный сервис</h3>
					<span class="state" data-running={installed && running}>
						{SERVICE_LABELS[run.service.state]}
					</span>
				</div>
				<code class="muted selectable">{run.service.name}</code>
			</header>

			<p class="muted hint">
				{#if installed}
					Запуск и остановка идут через диспетчер сервисов и прав администратора не требуют: они
					выданы вашей учётной записи при установке. Переустановка нужна, если сменился путь к
					файлу sing-box или к конфигу. После удаления sing-box будет запускаться обычным
					процессом — это работает для любого конфига без TUN.
				{:else if run.tun}
					Сервис обязателен: конфигу нужен TUN, а это права администратора. Установка запросит их
					один раз, дальше управление идёт без UAC.
				{:else}
					Этому конфигу сервис не нужен — TUN в нём нет, и sing-box запускается обычным процессом
					от вашего имени. Сервис пригодится, если позже добавите TUN или захотите, чтобы sing-box
					работал без запущенного Vantage Box.
				{/if}
			</p>

			<div class="actions">
				{#if installed}
					<button
						disabled={busy !== null || configMissing}
						onclick={() => act('install', api.installService)}
					>
						{busy === 'install' ? 'Переустанавливаю…' : 'Переустановить'}
					</button>
					<button disabled={busy !== null} onclick={() => act('uninstall', api.uninstallService)}>
						{busy === 'uninstall' ? 'Удаляю…' : 'Удалить'}
					</button>
				{:else}
					<button
						class:primary={run.tun}
						disabled={busy !== null || configMissing}
						onclick={() => act('install', api.installService)}
					>
						{busy === 'install' ? 'Устанавливаю…' : 'Установить сервис'}
					</button>
				{/if}
			</div>
		</section>
	{/if}

	<BinaryPanel />
</div>

<style>
	.page {
		display: grid;
		gap: 12px;
		align-content: start;
		max-width: 720px;
	}

	section {
		padding: 14px;
		display: grid;
		gap: 10px;
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}

	h3 {
		font-size: 14px;
	}

	.title {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.state {
		font-size: 12px;
		padding: 2px 8px;
		border-radius: 6px;
		background: var(--surface-alt);
		color: var(--text-muted);
	}

	.state[data-running='true'] {
		background: color-mix(in srgb, var(--good) 18%, transparent);
		color: var(--good);
	}

	.mode {
		font-size: 12px;
	}

	.actions {
		display: flex;
		gap: 8px;
		align-items: center;
		flex-wrap: wrap;
	}

	.hint {
		margin: 0;
		font-size: 12px;
	}

	.skipped {
		margin-top: 4px;
		font-size: 12px;
	}

	code {
		font-family: var(--mono);
		font-size: 12px;
	}
</style>
