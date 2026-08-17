<script lang="ts">
	import { api } from '$lib/api';
	import { runServiceAction } from '$lib/service-actions';
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
			await runServiceAction(name, call);
		} finally {
			busy = null;
		}
	}
</script>

<footer>
	<span class="dot" data-state={app.status.state}></span>
	<span class="state meta" title={label}>{label}</span>

	{#if app.status.version}
		<span class="meta muted selectable" title={`sing-box ${app.status.version}`}>
			sing-box {app.status.version}
		</span>
	{/if}

	{#if run}
		<span class="meta muted" title={run.mode === 'service' ? 'Запуск через службу Windows' : 'Запуск дочерним процессом'}>
			{run.mode === 'service' ? 'служба' : 'процесс'}
		</span>
	{/if}

	<span class="spacer"></span>

	{#if app.memory.inuse > 0}
		<span class="stat muted mono" title="Память sing-box">ОЗУ {formatBytes(app.memory.inuse)}</span>
	{/if}

	<!-- Ширина фиксирована: цифры меняются раз в секунду, и без неё соседние
		 элементы дёргались бы туда-сюда. -->
	<span class="rate mono" title="Скорость приёма"><span class="arrow">↓</span>{formatSpeed(app.traffic.down)}</span>
	<span class="rate mono" title="Скорость отдачи"><span class="arrow">↑</span>{formatSpeed(app.traffic.up)}</span>
	<span class="stat muted mono" title="Объём за текущий сеанс sing-box">Σ {formatBytes(total)}</span>

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
		/* Одна строка — переносы текста запрещены везде. */
		white-space: nowrap;
		overflow: hidden;
		min-width: 0;
	}

	.state {
		font-weight: 600;
	}

	/* Левая часть: статус и подписи. При нехватке места жмутся и обрезаются
	   многоточием; полный текст показывает системный тултип (title). Правая
	   часть (телеметрия, кнопки) приоритетнее — она не сжимается. */
	.meta {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.dot,
	.stat,
	.rate,
	.actions {
		flex-shrink: 0;
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
</style>
