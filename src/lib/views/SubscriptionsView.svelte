<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { app } from '$lib/state.svelte';
	import type { Settings, SubStateEntry, SubscriptionSettings } from '$lib/types';

	let draft = $state<Settings | null>(null);
	let saving = $state(false);
	let refreshing = $state(false);
	/** Состояние подписок из sidecar-файла: время/число узлов/ошибки. */
	let subState = $state<Record<string, SubStateEntry>>({});

	$effect(() => {
		// settings.json — источник правды. Правки в файле снаружи перебивают
		// незасейвленную форму.
		const current = app.settings;
		if (current) draft = structuredClone($state.snapshot(current)) as Settings;
	});

	const dirty = $derived(
		draft !== null &&
			app.settings !== null &&
			JSON.stringify($state.snapshot(draft)) !== JSON.stringify($state.snapshot(app.settings))
	);

	async function save() {
		if (!draft) return;
		saving = true;
		try {
			const next = $state.snapshot(draft) as Settings;
			// Пустая строка в поле группы означает «во все selector/urltest», а
			// бэкенд ждёт в этом случае null.
			next.subscriptions = next.subscriptions.map((s) => ({
				...s,
				targetGroup: s.targetGroup?.trim() ? s.targetGroup.trim() : null
			}));
			await app.saveSettings(next);
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			saving = false;
		}
	}

	function add() {
		if (!draft) return;
		draft.subscriptions = [
			...draft.subscriptions,
			{
				id: crypto.randomUUID(),
				name: '',
				url: '',
				enabled: true,
				targetGroup: null,
				updateInterval: 24
			} satisfies SubscriptionSettings
		];
	}

	function remove(id: string) {
		if (!draft) return;
		draft.subscriptions = draft.subscriptions.filter((s) => s.id !== id);
	}

	async function loadState() {
		try {
			const state = await api.getSubscriptionState();
			subState = state.entries ?? {};
		} catch {
			// Sidecar-файла может ещё не быть — молча.
		}
	}

	async function refreshNow() {
		if (!draft) return;
		if (dirty) {
			pushAlert('warn', 'Сначала сохраните изменения — обновление читает уже сохранённые подписки.');
			return;
		}
		refreshing = true;
		try {
			const outcome = await api.refreshSubscriptions(true);
			const total = outcome.updates.reduce((n, u) => n + u.nodeCount, 0);
			const failed = outcome.updates.filter((u) => u.lastError);
			if (failed.length > 0) {
				pushAlert('error', `Не удалось обновить: ${failed.map((u) => u.name || u.id).join(', ')}`);
			} else {
				pushAlert(
					'ok',
					`Влито узлов: ${total}.${outcome.restarted ? ' sing-box перезапущен.' : ' Конфиг обновлён без перезапуска.'}`
				);
			}
			await loadState();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			refreshing = false;
		}
	}

	/** Колонка узкая: год и секунды в ней всё равно не нужны. */
	function fmtTime(ms: number): string {
		if (!ms) return '—';
		try {
			return new Date(ms).toLocaleString(undefined, {
				day: '2-digit',
				month: '2-digit',
				hour: '2-digit',
				minute: '2-digit'
			});
		} catch {
			return '—';
		}
	}

	function tone(entry: SubStateEntry | undefined): 'good' | 'poor' | 'none' {
		if (!entry) return 'none';
		if (entry.lastError) return 'poor';
		return entry.nodeCount > 0 ? 'good' : 'none';
	}

	// Состояние подтягиваем при открытии вкладки и после каждого обновления.
	$effect(() => {
		if (app.settings) loadState();
	});
</script>

<div class="page">
	{#if draft}
		<div class="toolbar">
			<span class="count">{draft.subscriptions.length} подписок</span>
			<span
				class="hint ell"
				title="URL может отдавать конфиг sing-box с outbounds, голый массив outbound'ов или base64-список ss:// vmess:// vless:// trojan:// hysteria2:// tuic://"
			>
				URL отдаёт sing-box JSON или base64-список URI
			</span>
			<span class="spacer"></span>
			<button onclick={add}>
				<Icon name="plus" size={12} />
				Добавить
			</button>
			<button class="primary" onclick={refreshNow} disabled={refreshing}>
				{refreshing ? 'Обновляю…' : 'Обновить сейчас'}
			</button>
		</div>

		{#if draft.subscriptions.length === 0}
			<p class="hint">Подписок нет. «Добавить» создаёт новую — впишите URL, имя и группу.</p>
		{:else}
			<!-- Строки редактируются прямо в таблице: раньше каждая подписка была
				 карточкой на пять строк-лейблов, то есть ~230px под четыре поля. -->
			<div class="table card">
				<div class="row head">
					<span title="Подписка учитывается при обновлении"></span>
					<span>Имя</span>
					<span>URL</span>
					<span title="Пусто — узлы уйдут во все selector/urltest-группы">Группа</span>
					<span class="right" title="Интервал автообновления в часах">Ч</span>
					<span class="right">Узлов</span>
					<span>Обновлено</span>
					<span></span>
					<span></span>
				</div>

				{#each draft.subscriptions as sub (sub.id)}
					{@const st = subState[sub.id]}
					<div class="row">
						<input type="checkbox" bind:checked={sub.enabled} aria-label="Включена" />
						<input bind:value={sub.name} placeholder="имя" aria-label="Имя" />
						<input bind:value={sub.url} placeholder="https://…/sub" aria-label="URL" />
						<input
							bind:value={sub.targetGroup}
							placeholder="все группы"
							aria-label="Целевая группа"
						/>
						<input
							class="num"
							type="number"
							min="1"
							max="168"
							bind:value={sub.updateInterval}
							aria-label="Интервал обновления, часов"
						/>
						<span class="mono right muted">{st ? st.nodeCount : '—'}</span>
						<span class="mono muted ell">{st ? fmtTime(st.lastUpdated) : '—'}</span>
						<span
							class="dot"
							data-tone={tone(st)}
							title={st?.lastError ?? (st ? `узлов: ${st.nodeCount}` : 'ещё не обновлялась')}
						></span>
						<button
							class="icon-btn"
							title="Удалить подписку"
							aria-label="Удалить подписку"
							onclick={() => remove(sub.id)}
						>
							<Icon name="trash" size={12} />
						</button>
					</div>
				{/each}
			</div>
		{/if}

		<p class="hint">
			Узлы вливаются в config.json под тегами <code class="inline">sub:</code> и дописываются в
			целевые группы; при обновлении старые снимаются, поэтому дубликатов не возникает.
			Комментарии в config.json не сохраняются — как и в редакторе конфига.
		</p>

		<div class="sticky-footer">
			<button class="primary" onclick={save} disabled={!dirty || saving}>
				{saving ? 'Сохраняю…' : 'Сохранить'}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving}>Отменить</button>
			{#if dirty}<span class="hint">есть несохранённые изменения</span>{/if}
		</div>
	{:else}
		<p class="hint">Загружаю настройки…</p>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		min-height: 100%;
	}

	.count {
		font-weight: 600;
		white-space: nowrap;
	}

	.toolbar button {
		display: inline-flex;
		align-items: center;
		gap: var(--sp-2);
	}

	.table {
		display: grid;
		align-content: start;
		overflow-x: auto;
	}

	.row {
		display: grid;
		grid-template-columns:
			16px minmax(80px, 1fr) minmax(140px, 2.4fr) minmax(80px, 1fr)
			calc(var(--w-num) + var(--sp-4)) 44px 96px 10px var(--h-ctl);
		align-items: center;
		gap: var(--sp-4);
		padding: var(--sp-1) var(--sp-2) var(--sp-1) var(--sp-3);
		font-size: var(--fs-sm);
		min-width: 620px;
	}

	.row:not(.head):hover {
		background: var(--surface-alt);
	}

	.head {
		position: sticky;
		top: 0;
		z-index: 1;
		height: var(--h-row);
		padding-top: 0;
		padding-bottom: 0;
		background: var(--surface);
		border-bottom: 1px solid var(--border);
		color: var(--text-muted);
		font-size: var(--fs-xs);
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.row input:not([type='checkbox']) {
		width: 100%;
		font-size: var(--fs-sm);
		background: transparent;
		border-color: transparent;
	}

	/* Поле выглядит текстом, пока в него не целятся: таблица должна читаться
	   как таблица, а не как форма из девяти рамок в каждой строке. */
	.row input:not([type='checkbox']):hover,
	.row input:not([type='checkbox']):focus {
		background: var(--surface-alt);
		border-color: var(--border);
	}

	.right {
		text-align: right;
	}
</style>
