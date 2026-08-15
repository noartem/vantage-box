<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { app } from '$lib/state.svelte';
	import { formatBytes, formatSpeed } from '$lib/format';

	let busy = $state<string | null>(null);
	let actionError = $state<string | null>(null);

	const label = $derived(
		{
			connected: 'подключено',
			connecting: 'подключение',
			disconnected: 'нет связи'
		}[app.status.state]
	);

	const run = $derived(app.run);
	const running = $derived(run?.running === true);
	/** Конфигу нужен TUN, а сервис не установлен — запускать нечем. */
	const blocked = $derived(run !== null && run.mode === 'process' && run.tun);
	const total = $derived(app.totals.up + app.totals.down);

	async function act(name: string, call: () => Promise<unknown>) {
		busy = name;
		actionError = null;
		try {
			await call();
		} catch (e) {
			actionError = errorText(e);
		} finally {
			busy = null;
			await app.refreshRun();
		}
	}
</script>

<!-- Один общий контейнер: компонент вставляется в grid родителя, и отдельные
	 баннеры иначе превращались бы в лишние строки его сетки. -->
<div class="bar">
	<header>
		<div class="left">
			<span class="dot" data-state={app.status.state}></span>
			<span class="state">{label}</span>
			{#if app.status.version}
				<span class="muted selectable">sing-box {app.status.version}</span>
			{/if}
		</div>

		<div class="right">
			<div class="traffic" title="Скорость сейчас и объём за текущий сеанс sing-box">
				<span class="rate"><span class="arrow">↓</span>{formatSpeed(app.traffic.down)}</span>
				<span class="rate"><span class="arrow">↑</span>{formatSpeed(app.traffic.up)}</span>
				<span class="muted">всего {formatBytes(total)}</span>
			</div>

			<div class="actions">
				<button
					class="primary"
					disabled={busy !== null || running || blocked}
					onclick={() => act('start', api.start)}
				>
					{busy === 'start' ? 'Запускаю…' : 'Запустить'}
				</button>
				<button disabled={busy !== null || !running} onclick={() => act('stop', api.stop)}>
					{busy === 'stop' ? 'Останавливаю…' : 'Остановить'}
				</button>
				<button disabled={busy !== null || !running} onclick={() => act('restart', api.restart)}>
					{busy === 'restart' ? 'Перезапускаю…' : 'Перезапустить'}
				</button>
			</div>
		</div>
	</header>

	{#if blocked}
		<div class="banner warn">
			В конфиге есть TUN-инбаунд — для него нужны права администратора. Установите сервис на
			вкладке «Сервис».
		</div>
	{/if}

	{#if app.status.state === 'disconnected' && app.status.error && running}
		<div class="banner">
			<strong>sing-box запущен, но Clash API не отвечает.</strong>
			{app.status.error}
		</div>
	{/if}

	{#if actionError}
		<div class="banner">{actionError}</div>
	{/if}

	{#if app.status.compatibility === 'tooNew' || app.status.compatibility === 'tooOld'}
		<div class="banner warn">
			Версия sing-box {app.status.version} вне протестированного диапазона. Приложение работает, но
			часть возможностей может вести себя иначе.
		</div>
	{/if}

	{#if app.settingsProblem}
		<div class="banner">
			<strong>settings.json:</strong>
			{app.settingsProblem}
		</div>
	{/if}

	{#if app.updateAvailable}
		<div class="banner ok update">
			<span>
				<strong>Доступно обновление {app.updateAvailable.version}.</strong>
				<a
					href="https://github.com/noartem/vantage-box/releases"
					target="_blank"
					rel="noopener">Что нового</a
				>
			</span>
			<button
				class="primary"
				disabled={app.updateInstalling}
				onclick={() => app.installAppUpdate()}
			>
				{app.updateInstalling ? 'Устанавливаю…' : 'Установить и перезапустить'}
			</button>
		</div>
	{/if}
</div>

<style>
	.bar {
		display: grid;
		gap: 8px;
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		flex-wrap: wrap;
	}

	.left,
	.right {
		display: flex;
		align-items: center;
		gap: 12px;
	}

	.state {
		font-weight: 600;
	}

	.traffic {
		display: flex;
		align-items: baseline;
		gap: 10px;
		font-size: 12px;
	}

	/* Ширина фиксирована: цифры меняются раз в секунду, и без неё соседние
	   элементы дёргались бы туда-сюда. */
	.rate {
		font-family: var(--mono);
		min-width: 92px;
		text-align: right;
	}

	.arrow {
		color: var(--text-muted);
		margin-right: 3px;
	}

	.actions {
		display: flex;
		gap: 8px;
	}

	.update {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		flex-wrap: wrap;
	}

	.dot {
		width: 9px;
		height: 9px;
		border-radius: 50%;
		background: var(--text-muted);
	}

	.dot[data-state='connected'] {
		background: var(--good);
	}

	.dot[data-state='connecting'] {
		background: var(--fair);
	}

	.dot[data-state='disconnected'] {
		background: var(--poor);
	}
</style>
