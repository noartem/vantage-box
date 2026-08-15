<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { app } from '$lib/state.svelte';
	import type { Settings, SubStateEntry, SubscriptionSettings } from '$lib/types';

	let draft = $state<Settings | null>(null);
	let error = $state<string | null>(null);
	let info = $state<string | null>(null);
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
			JSON.stringify($state.snapshot(draft)) !==
				JSON.stringify($state.snapshot(app.settings))
	);

	async function save() {
		if (!draft) return;
		saving = true;
		error = null;
		try {
			await app.saveSettings($state.snapshot(draft) as Settings);
		} catch (e) {
			error = errorText(e);
		} finally {
			saving = false;
		}
	}

	function newSubscription(): SubscriptionSettings {
		return {
			id: crypto.randomUUID(),
			name: '',
			url: '',
			enabled: true,
			targetGroup: null,
			updateInterval: 24
		};
	}

	function add() {
		if (!draft) return;
		draft.subscriptions = [...draft.subscriptions, newSubscription()];
	}

	function remove(id: string) {
		if (!draft) return;
		draft.subscriptions = draft.subscriptions.filter((s) => s.id !== id);
	}

	async function loadState() {
		try {
			const s = await api.getSubscriptionState();
			subState = s.entries ?? {};
		} catch {
			// Sidecar-файла может ещё не быть — молча.
		}
	}

	async function refreshNow() {
		if (!draft) return;
		if (dirty) {
			error = 'Сначала сохраните изменения — обновление читает уже сохранённые подписки.';
			return;
		}
		refreshing = true;
		error = null;
		info = null;
		try {
			const outcome = await api.refreshSubscriptions(true);
			const total = outcome.updates.reduce((n, u) => n + u.nodeCount, 0);
			const failed = outcome.updates.filter((u) => u.lastError);
			if (failed.length > 0) {
				error = `Не удалось обновить: ${failed.map((u) => u.name || u.id).join(', ')}`;
			} else {
				info = `Влито узлов: ${total}.${
					outcome.restarted ? ' sing-box перезапущен.' : ' Конфиг обновлён без перезапуска.'
				}`;
			}
			await loadState();
		} catch (e) {
			error = errorText(e);
		} finally {
			refreshing = false;
		}
	}

	function fmtTime(ms: number): string {
		if (!ms) return '—';
		try {
			return new Date(ms).toLocaleString();
		} catch {
			return '—';
		}
	}

	// Состояние подтягиваем при открытии вкладки и после каждого обновления.
	$effect(() => {
		if (app.settings) loadState();
	});
</script>

<div class="page">
	{#if draft}
		<section class="card toolbar">
			<div class="head">
				<h3>Подписки</h3>
				<span class="muted">URL отдаёт sing-box JSON или base64-список URI</span>
			</div>
			<div class="actions">
				<button onclick={add}>Добавить</button>
				<button class="primary" onclick={refreshNow} disabled={refreshing}>
					{refreshing ? 'Обновляю…' : 'Обновить сейчас'}
				</button>
			</div>
		</section>

		{#if error}
			<div class="banner warn">{error}</div>
		{/if}
		{#if info}
			<div class="banner ok">{info}</div>
		{/if}

		{#if draft.subscriptions.length === 0}
			<section class="card">
				<p class="muted">Подписок нет. «Добавить» создаёт новую — впишите URL, имя и группу.</p>
			</section>
		{:else}
			{#each draft.subscriptions as sub (sub.id)}
				{@const st = subState[sub.id]}
				<section class="card sub">
					<div class="sub-head">
						<label class="row">
							<input type="checkbox" bind:checked={sub.enabled} />
							<span>включена</span>
						</label>
						<button onclick={() => remove(sub.id)}>Удалить</button>
					</div>
					<label>
						<span>Имя</span>
						<input bind:value={sub.name} placeholder="моя подписка" />
					</label>
					<label>
						<span>URL</span>
						<input bind:value={sub.url} placeholder="https://…/sub" />
					</label>
					<label>
						<span>Группа</span>
						<input
							bind:value={sub.targetGroup}
							placeholder="пусто — во все selector/urltest"
						/>
					</label>
					<label>
						<span>Интервал, ч</span>
						<input type="number" min="1" max="168" bind:value={sub.updateInterval} />
					</label>
					{#if st}
						<div class="state">
							<span class="muted">узлов: {st.nodeCount}</span>
							<span class="muted">обновлено: {fmtTime(st.lastUpdated)}</span>
							{#if st.lastError}
								<span class="err">{st.lastError}</span>
							{/if}
						</div>
					{/if}
				</section>
			{/each}
		{/if}

		<p class="muted hint">
			Узлы вливаются в config.json под тегами с префиксом <code>sub:</code> и дописываются в
			целевые selector/urltest-группы. При обновлении старые узлы подписки снимаются и
			накатываются заново, поэтому дубликатов не возникает. Комментарии в config.json при этом
			не сохраняются — как и в редакторе конфига.
		</p>

		<div class="footer">
			<button class="primary" onclick={save} disabled={!dirty || saving}>
				{saving ? 'Сохраняю…' : 'Сохранить'}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving}>Отменить</button>
			{#if dirty}<span class="muted">есть несохранённые изменения</span>{/if}
		</div>
	{:else}
		<p class="muted">Загружаю настройки…</p>
	{/if}
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

	.toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		flex-wrap: wrap;
	}

	.head {
		display: flex;
		align-items: baseline;
		gap: 12px;
	}

	.actions {
		display: flex;
		gap: 8px;
	}

	h3 {
		font-size: 14px;
		margin: 0;
	}

	label {
		display: grid;
		grid-template-columns: 120px 1fr;
		align-items: center;
		gap: 10px;
	}

	label.row {
		grid-template-columns: auto 1fr;
		justify-items: start;
	}

	.sub-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.state {
		display: flex;
		flex-wrap: wrap;
		gap: 14px;
		font-size: 12px;
	}

	.err {
		color: var(--danger, #d33);
	}

	.hint {
		font-size: 12px;
	}

	.hint code {
		font-family: var(--mono);
		background: var(--surface-alt);
		padding: 1px 4px;
		border-radius: 4px;
	}

	.footer {
		display: flex;
		align-items: center;
		gap: 10px;
		position: sticky;
		bottom: 0;
		padding: 10px 0;
		background: var(--bg);
	}
</style>