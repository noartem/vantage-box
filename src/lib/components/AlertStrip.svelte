<script lang="ts">
	import Icon from './Icon.svelte';
	import { dismissAlert, transientAlerts, type AlertSeverity } from '$lib/alerts.svelte';
	import { app } from '$lib/state.svelte';
	import type { TabId } from '$lib/tabs';

	let { ongoto }: { ongoto: (tab: TabId) => void } = $props();

	type Item = {
		key: string;
		severity: AlertSeverity;
		text: string;
		action?: { label: string; disabled?: boolean; run: () => void };
	};

	/** Раньше каждая проблема была отдельным баннером в шапке, и верхняя строка
	 *  окна прыгала с 33px до 250px. Теперь все они — один список, из которого
	 *  видна ровно одна запись: высота полосы постоянна, контент не съезжает. */
	const items = $derived.by<Item[]>(() => {
		const list: Item[] = [];

		if (app.settingsProblem) {
			list.push({
				key: 'settings',
				severity: 'error',
				text: `settings.json: ${app.settingsProblem}`,
				action: { label: 'Настройки', run: () => ongoto('settings') }
			});
		}

		if (app.status.state === 'disconnected' && app.status.error && app.run?.running) {
			list.push({
				key: 'api',
				severity: 'error',
				text: `sing-box запущен, но Clash API не отвечает. ${app.status.error}`,
				action: { label: 'Конфиг', run: () => ongoto('config') }
			});
		}

		if (app.run?.configProblem) {
			list.push({
				key: 'config',
				severity: 'error',
				text: `Не удалось прочитать конфиг: ${app.run.configProblem}`,
				action: { label: 'Конфиг', run: () => ongoto('config') }
			});
		}

		for (const alert of transientAlerts.items) {
			list.push({
				key: `t${alert.id}`,
				severity: alert.severity,
				text: alert.text,
				action: { label: 'Скрыть', run: () => dismissAlert(alert.id) }
			});
		}

		// Конфигу нужен TUN, а служба не установлена — запускать нечем.
		if (app.run?.mode === 'process' && app.run.tun) {
			list.push({
				key: 'tun',
				severity: 'warn',
				text: 'В конфиге есть TUN-инбаунд — ему нужны права администратора. Установите службу.',
				action: { label: 'Сервис', run: () => ongoto('service') }
			});
		}

		if (app.status.compatibility === 'tooNew' || app.status.compatibility === 'tooOld') {
			list.push({
				key: 'compat',
				severity: 'warn',
				text: `Версия sing-box ${app.status.version ?? ''} вне протестированного диапазона: приложение работает, но часть возможностей может вести себя иначе.`
			});
		}

		if (app.updateAvailable) {
			list.push({
				key: 'update',
				severity: 'ok',
				text: `Доступно обновление Vantage Box ${app.updateAvailable.version}.`,
				action: {
					label: app.updateInstalling ? 'Устанавливаю…' : 'Обновить',
					disabled: app.updateInstalling,
					run: () => app.installAppUpdate()
				}
			});
		}

		return list;
	});

	let cursor = $state(0);
	const position = $derived(items.length === 0 ? 0 : Math.min(cursor, items.length - 1));
	const current = $derived(items[position]);

	function step(delta: number) {
		cursor = (position + delta + items.length) % items.length;
	}
</script>

{#if current}
	<div class="strip" data-severity={current.severity}>
		<Icon name={current.severity === 'ok' ? 'info' : 'alert'} size={13} />

		<span class="text ell selectable" title={current.text}>{current.text}</span>

		{#if current.action}
			<button class="act" disabled={current.action.disabled} onclick={current.action.run}>
				{current.action.label}
			</button>
		{/if}

		{#if items.length > 1}
			<button class="icon-btn" aria-label="Предыдущее сообщение" onclick={() => step(-1)}>
				<Icon name="chevronLeft" size={12} />
			</button>
			<span class="count mono">{position + 1}/{items.length}</span>
			<button class="icon-btn" aria-label="Следующее сообщение" onclick={() => step(1)}>
				<Icon name="chevronRight" size={12} />
			</button>
		{/if}
	</div>
{/if}

<style>
	.strip {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-alert);
		padding: 0 var(--sp-4);
		font-size: var(--fs-sm);
		border-bottom: 1px solid var(--border);
		color: var(--poor);
		background: color-mix(in srgb, var(--poor) 12%, var(--bg));
	}

	.strip[data-severity='warn'] {
		color: var(--fair);
		background: color-mix(in srgb, var(--fair) 12%, var(--bg));
	}

	.strip[data-severity='ok'] {
		color: var(--good);
		background: color-mix(in srgb, var(--good) 12%, var(--bg));
	}

	/* Текст занимает всё свободное место и обрезается; целиком он есть в title —
	   так строка не может вырасти во вторую. */
	.text {
		flex: 1;
	}

	/* Кнопка внутри полосы ниже обычной: 22px не влезают в 24px строку. */
	.act {
		height: 18px;
		padding: 0 var(--sp-3);
		font-size: var(--fs-xs);
		background: transparent;
		border-color: currentcolor;
		color: inherit;
	}

	.act:hover:not(:disabled) {
		background: color-mix(in srgb, currentcolor 15%, transparent);
		border-color: currentcolor;
	}

	.strip .icon-btn {
		width: 18px;
		height: 18px;
		color: inherit;
	}

	.count {
		font-size: var(--fs-xs);
		opacity: 0.8;
	}
</style>
