<script lang="ts">
	import { api } from '$lib/api';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import VersionsPanel from '$lib/components/VersionsPanel.svelte';
	import { SERVICE_LABELS, runServiceAction } from '$lib/service-actions';
	import { app } from '$lib/state.svelte';

	let busy = $state<string | null>(null);
	let help = $state(false);

	const run = $derived(app.run);
	const installed = $derived(run !== null && run.mode === 'service');
	const running = $derived(run?.running === true);
	/** Сервисная служба именно работает — переустановка/удаление в этот момент
	 *  рвут VPN, поэтому кнопки блокируются. */
	const serviceRunning = $derived(run?.service.state === 'running');
	/** Конфигу нужен TUN, а сервиса нет: запускать нечем, установка обязательна. */
	const serviceRequired = $derived(run !== null && !installed && run.tun);
	const configPath = $derived(app.settings?.singBox.configPath ?? '');
	const configMissing = $derived(configPath.trim() === '');

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}
</script>

<div class="page">
	<!-- Карточки идут потоком по колонкам: в гриде строка была высотой с самую
		 высокую секцию, и под короткой оставалась пустота до конца ряда. -->
	<div class="masonry">
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
						<span class="tip">
							<button
								class:primary={run.tun && !installed}
								disabled={busy !== null || serviceRunning || configMissing}
								onclick={() => act('install', api.installService)}
							>
								{#if busy === 'install'}
									{installed ? 'Переустанавливаю…' : 'Устанавливаю…'}
								{:else}
									{installed ? 'Переустановить' : 'Установить службу'}
								{/if}
							</button>
							{#if serviceRunning}
								<span class="tip-balloon">
									Служба работает — переустановка и удаление заблокированы, чтобы не рвать
									VPN. Сначала остановите sing-box.
								</span>
							{/if}
						</span>
						{#if installed}
							<span class="tip">
								<button
									class="danger"
									disabled={busy !== null || serviceRunning}
									onclick={() => act('uninstall', api.uninstallService)}
								>
									{busy === 'uninstall' ? 'Удаляю…' : 'Удалить'}
								</button>
								{#if serviceRunning}
									<span class="tip-balloon">
										Служба работает — переустановка и удаление заблокированы, чтобы не рвать
										VPN. Сначала остановите sing-box.
									</span>
								{/if}
							</span>
						{/if}
					</div>

					{#if help}
						<p class="hint">
							{#if installed}
								Запуск и остановка идут через диспетчер служб и прав администратора не требуют: они
								выданы вашей учётной записи при установке. Переустановка нужна, если сменился путь к
								файлу sing-box или к конфигу. Пока служба работает, переустановка и удаление заблокированы
								— сначала остановите sing-box, иначе VPN порвётся. После удаления sing-box будет
								запускаться обычным процессом — это работает для любого конфига без TUN.
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

	<!-- Каталог версий — таблица: в колонке 330px он был бы нечитаем, поэтому
		 живёт под плиткой во всю ширину. -->
	<VersionsPanel />
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
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
