<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import InfoButton from '$lib/components/InfoButton.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { Settings, SubStateEntry, SubscriptionSettings } from '$lib/types';

	let draft = $state<Settings | null>(null);
	let saving = $state(false);
	let applying = $state(false);
	/** Subscription state from the sidecar file: time/node count/errors. */
	let subState = $state<Record<string, SubStateEntry>>({});
	/** Saved subscription metadata not yet injected into the running config. */
	let applyPending = $state(false);

	$effect(() => {
		// settings.json is the source of truth. Edits to the file from outside
		// override the unsaved form.
		const current = app.settings;
		if (current) draft = structuredClone($state.snapshot(current)) as Settings;
	});

	const dirty = $derived(
		draft !== null &&
			app.settings !== null &&
			JSON.stringify($state.snapshot(draft)) !== JSON.stringify($state.snapshot(app.settings))
	);

	/** Normalize the draft and write it to settings.json. Does not touch the
	 *  running config or restart sing-box. */
	async function persist(): Promise<void> {
		if (!draft) return;
		const next = $state.snapshot(draft) as Settings;
		// An empty string in the group field means "into all selector/urltest",
		// and the backend expects null in that case.
		next.subscriptions = next.subscriptions.map((s) => ({
			...s,
			targetGroup: s.targetGroup?.trim() ? s.targetGroup.trim() : null
		}));
		await app.saveSettings(next);
	}

	async function save() {
		if (!draft || saving || applying) return;
		saving = true;
		try {
			await persist();
			// persist wrote new subscription metadata to disk and flagged it
			// pending in the sidecar — reload so the Apply button reflects that.
			await loadState();
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
			applyPending = state.applyPending ?? false;
		} catch {
			// The sidecar file may not exist yet — stay silent.
		}
	}

	/** Fetch every enabled subscription, inject nodes into the running config,
	 *  and restart sing-box. Reads URLs from already-saved settings.json —
	 *  which is why the caller persists first. */
	async function fetchAndApply(): Promise<void> {
		const outcome = await api.refreshSubscriptions(true);
		const total = outcome.updates.reduce((n, u) => n + u.nodeCount, 0);
		const failed = outcome.updates.filter((u) => u.lastError);
		if (failed.length > 0) {
			pushAlert('error', m.subs_refresh_failed({ names: failed.map((u) => u.name || u.id).join(', ') }));
		} else {
			const suffix = outcome.restarted ? m.subs_restarted_suffix() : m.subs_no_restart_suffix();
			pushAlert('ok', `${m.subs_nodes_poured({ count: total })}${suffix}`);
		}
		await loadState();
	}

	// "Apply & Save": persist any unsaved metadata first, then fetch nodes and
	// inject them into the running config (restarting sing-box). Saving first
	// is what lets the backend read the URL/group list you just edited.
	async function applyAndSave() {
		if (!draft || saving || applying) return;
		applying = true;
		try {
			if (dirty) await persist();
			await fetchAndApply();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			applying = false;
		}
	}

	/** The column is narrow: year and seconds are not needed here anyway. */
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

	// We pull state on tab open and after each refresh.
	$effect(() => {
		if (app.settings) loadState();
	});
</script>

<div class="page">
	{#if draft}
		<div class="toolbar">
			<span class="count">{m.subs_count({ count: draft.subscriptions.length })}</span>
			<span class="spacer"></span>
			<InfoButton label={() => m.common_explanations()}>
				<p>{m.subs_url_hint()}</p>
				<p>
					{m.subs_hint_pre()}
					<code class="inline">sub:</code>
					{m.subs_hint_post()}
				</p>
			</InfoButton>
			<button onclick={add}>
				<Icon name="plus" size={12} />
				{m.subs_add()}
			</button>
		</div>

		<!-- Rows are edited right in the table: previously each subscription was
			 a card with five label rows, i.e. ~230px for four fields. -->
		<div class="table card">
			<div class="row head">
				<span title={m.subs_enabled_hint()}></span>
				<span>{m.common_name()}</span>
				<span>URL</span>
				<span title={m.subs_group_hint()}>{m.subs_group()}</span>
				<span class="right" title={m.subs_interval_hint()}>{m.subs_interval_h()}</span>
				<span class="right">{m.subs_nodes()}</span>
				<span>{m.subs_updated()}</span>
				<span></span>
				<span></span>
			</div>

			{#if draft.subscriptions.length === 0}
				<div class="row empty-row">
					<div class="empty">
						<Icon name="subscriptions" size={48} />
						<p class="empty-title">{m.subs_empty_title()}</p>
						<p class="hint">{m.subs_empty()}</p>
						<button onclick={add}>
							<Icon name="plus" size={12} />
							{m.subs_add()}
						</button>
					</div>
				</div>
			{:else}
				{#each draft.subscriptions as sub (sub.id)}
					{@const st = subState[sub.id]}
					<div class="row">
						<input type="checkbox" bind:checked={sub.enabled} aria-label={m.subs_enabled_label()} />
						<input bind:value={sub.name} placeholder={m.subs_name_placeholder()} aria-label={m.common_name()} />
						<input bind:value={sub.url} placeholder="https://…/sub" aria-label="URL" />
						<input
							bind:value={sub.targetGroup}
							placeholder={m.subs_group_placeholder()}
							aria-label={m.subs_target_group()}
						/>
						<input
							class="num"
							type="number"
							min="1"
							max="168"
							bind:value={sub.updateInterval}
							aria-label={m.subs_interval_label()}
						/>
						<span class="mono right muted">{st ? st.nodeCount : '—'}</span>
						<span class="mono muted ell">{st ? fmtTime(st.lastUpdated) : '—'}</span>
						<span
							class="dot"
							data-tone={tone(st)}
							title={st?.lastError ?? (st ? m.subs_nodes_count({ count: st.nodeCount }) : m.subs_not_refreshed())}
						></span>
						<button
							class="icon-btn"
							title={m.subs_delete_title()}
							aria-label={m.subs_delete_title()}
							onclick={() => remove(sub.id)}
						>
							<Icon name="trash" size={12} />
						</button>
					</div>
				{/each}
			{/if}
		</div>

		<div class="sticky-footer">
			<button
				class="primary"
				onclick={applyAndSave}
				disabled={(!dirty && !applyPending) || saving || applying}
			>
				{applying ? m.subs_applying() : dirty ? m.subs_apply_save() : m.subs_apply()}
			</button>
			<button onclick={save} disabled={!dirty || saving || applying}>
				{saving ? m.common_saving() : m.common_save()}
			</button>
			<button onclick={() => app.refreshSettings()} disabled={!dirty || saving || applying}>
				{m.common_cancel()}
			</button>
			{#if dirty}<span class="hint">{m.common_unsaved_changes()}</span>{/if}
		</div>
	{:else}
		<p class="hint">{m.common_loading_settings()}</p>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		min-height: 100%;
	}

	.empty {
		grid-column: 1 / -1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: var(--sp-4);
		padding: var(--sp-6) 0;
		text-align: center;
	}

	.empty :global(svg) {
		color: var(--text-muted);
		opacity: 0.45;
	}

	.empty-title {
		font-size: var(--fs-lg);
		font-weight: 600;
		color: var(--text);
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

	/* A field looks like text until it is focused: the table should read as a
	   table, not as a form with nine borders in every row. */
	.row input:not([type='checkbox']):hover,
	.row input:not([type='checkbox']):focus {
		background: var(--surface-alt);
		border-color: var(--border);
	}

	.right {
		text-align: right;
	}
</style>
