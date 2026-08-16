<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { app } from '$lib/state.svelte';
	import { formatBytes, formatSpeed } from '$lib/format';
	import Icon from './Icon.svelte';

	let busy = $state<string | null>(null);

	const label = $derived(
		{
			connected: 'подключено',
			connecting: 'подключение',
			disconnected: 'нет связи'
		}[app.status.state]
	);

	const run = $derived(app.run);
	const running = $derived(run?.running === true);
	/** Конфигу нужен TUN, а служба не установлена — запускать нечем. */
	const blocked = $derived(run !== null && run.mode === 'process' && run.tun);
	const total = $derived(app.totals.up + app.totals.down);

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		try {
			await call();
		} catch (e) {
			// Ошибка действия — не состояние, а разовое событие: она уходит в общую
			// строку алертов, а не растит эту полосу второй строкой.
			pushAlert('error', errorText(e));
		} finally {
			busy = null;
			await app.refreshRun();
		}
	}
</script>

<footer>
	<span class="dot" data-state={app.status.state}></span>
	<span class="state">{label}</span>

	{#if app.status.version}
		<span class="muted selectable">sing-box {app.status.version}</span>
	{/if}

	{#if run}
		<span class="muted" title={run.mode === 'service' ? 'Запуск через службу Windows' : 'Запуск дочерним процессом'}>
			{run.mode === 'service' ? 'служба' : 'процесс'}
		</span>
	{/if}

	<span class="spacer"></span>

	{#if app.memory.inuse > 0}
		<span class="muted mono" title="Память sing-box">ОЗУ {formatBytes(app.memory.inuse)}</span>
	{/if}

	<!-- Ширина фиксирована: цифры меняются раз в секунду, и без неё соседние
		 элементы дёргались бы туда-сюда. -->
	<span class="rate mono" title="Скорость приёма"><span class="arrow">↓</span>{formatSpeed(app.traffic.down)}</span>
	<span class="rate mono" title="Скорость отдачи"><span class="arrow">↑</span>{formatSpeed(app.traffic.up)}</span>
	<span class="muted mono" title="Объём за текущий сеанс sing-box">Σ {formatBytes(total)}</span>

	<div class="actions">
		<button
			class="icon-btn"
			title={blocked ? 'Нужна служба: в конфиге TUN-инбаунд' : 'Запустить sing-box'}
			aria-label="Запустить"
			disabled={busy !== null || running || blocked}
			onclick={() => act('start', api.start)}
		>
			<Icon name="play" size={12} fill />
		</button>
		<button
			class="icon-btn"
			title="Остановить sing-box"
			aria-label="Остановить"
			disabled={busy !== null || !running}
			onclick={() => act('stop', api.stop)}
		>
			<Icon name="stop" size={12} fill />
		</button>
		<button
			class="icon-btn"
			title="Мягкий перезапуск с восстановлением выбранных узлов"
			aria-label="Перезапустить"
			disabled={busy !== null || !running}
			onclick={() => act('restart', api.restart)}
		>
			<Icon name="restart" size={13} />
		</button>
	</div>
</footer>

<style>
	footer {
		display: flex;
		align-items: center;
		gap: var(--sp-4);
		height: var(--h-status);
		padding: 0 var(--sp-3) 0 var(--sp-4);
		font-size: var(--fs-sm);
		background: var(--surface);
		border-top: 1px solid var(--border);
		flex-shrink: 0;
	}

	.state {
		font-weight: 600;
	}

	.rate {
		min-width: 78px;
		text-align: right;
	}

	.arrow {
		color: var(--text-muted);
		margin-right: 2px;
	}

	.actions {
		display: flex;
		gap: var(--sp-1);
		margin-left: var(--sp-2);
	}

	/* На узком окне телеметрия уступает место статусу и кнопкам. */
	@media (max-width: 760px) {
		.rate {
			min-width: 0;
		}
	}
</style>
