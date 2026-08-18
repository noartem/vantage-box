<script lang="ts">
	import { onMount } from 'svelte';
	import { emit } from '@tauri-apps/api/event';
	import { api, errorText, events } from '$lib/api';
	import { delayTone, formatDelay } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import type { ConnectionStatus, GroupView } from '$lib/types';

	/** The node count past which the list stops being readable by eye. */
	const FILTER_FROM = 12;

	// The popup lives in a separate webview and intentionally does not spin up the
	// shared app state: it only needs the list of groups, and it must open instantly.
	let groups = $state<GroupView[]>([]);
	let status = $state<ConnectionStatus | null>(null);
	let error = $state<string | null>(null);
	let pending = $state<string | null>(null);
	let filter = $state('');

	const totalNodes = $derived(groups.reduce((n, group) => n + group.items.length, 0));

	const shown = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		if (q === '') return groups;
		return groups
			.map((group) => ({
				...group,
				items: group.items.filter((item) => item.name.toLowerCase().includes(q))
			}))
			.filter((group) => group.items.length > 0);
	});

	async function load() {
		try {
			const [overview, connection] = await Promise.all([api.getProxies(), api.getStatus()]);
			groups = overview.groups.filter((group) => group.selectable);
			status = connection;
			error = null;
		} catch (e) {
			error = errorText(e);
		}
	}

	async function select(group: GroupView, node: string) {
		if (node === group.now) {
			await api.closePopup();
			return;
		}
		pending = `${group.name} ${node}`;
		try {
			await api.selectProxy(group.name, node);
			await api.closePopup();
		} catch (e) {
			error = errorText(e);
		} finally {
			pending = null;
		}
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') api.closePopup();
	}

	onMount(() => {
		load();
		events.status((value) => (status = value));
		// Signal for `--self-test`: the popup webview really did load and run
		// our code. There is no other way to check this from the outside.
		emit('popup://ready');
	});
</script>

<svelte:window onkeydown={onKeydown} />

<div class="popup">
	<header>
		<span class="dot" data-state={status?.state ?? 'connecting'}></span>
		<span class="title">Vantage Box</span>
		<button class="link" onclick={() => api.showMainWindow()}>{m.popup_open_window()}</button>
	</header>

	{#if totalNodes >= FILTER_FROM}
		<div class="search">
			<!-- svelte-ignore a11y_autofocus -->
			<input
				class="grow"
				type="search"
				placeholder={m.popup_filter_placeholder()}
				aria-label={m.popup_filter_label()}
				autofocus
				bind:value={filter}
			/>
		</div>
	{/if}

	<div class="body bounce">
		{#if error}
			<div class="banner">{error}</div>
		{:else if groups.length === 0}
			<p class="hint empty">
				{status?.state === 'connected' ? m.popup_no_groups() : m.popup_no_api()}
			</p>
		{:else if shown.length === 0}
			<p class="hint empty">{m.popup_nothing_found()}</p>
		{:else}
			{#each shown as group (group.name)}
				<section>
					<h2 class="section-title">{group.name}</h2>
					{#each group.items as item (item.name)}
						<button
							class="node"
							class:active={item.name === group.now}
							disabled={pending !== null}
							onclick={() => select(group, item.name)}
							title={item.kind}
						>
							<span class="ell">{item.name}</span>
							<span class="delay" data-tone={delayTone(item.delay)}>
								{formatDelay(item.delay)}
							</span>
						</button>
					{/each}
				</section>
			{/each}
		{/if}
	</div>

	<footer class="hint">{m.popup_esc_close()}</footer>
</div>

<style>
	.popup {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-card);
		overflow: hidden;
	}

	header {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		padding: 0 var(--sp-3) 0 var(--sp-4);
		height: var(--h-status);
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.title {
		font-weight: 600;
		font-size: var(--fs-sm);
		flex: 1;
	}

	.link {
		background: transparent;
		border-color: transparent;
		color: var(--accent);
		padding: 0 var(--sp-2);
		font-size: var(--fs-sm);
	}

	.search {
		padding: var(--sp-2) var(--sp-2) 0;
		flex-shrink: 0;
	}

	.body {
		flex: 1;
		overflow-y: auto;
		padding: var(--sp-2);
		min-height: 0;
	}

	section + section {
		margin-top: var(--sp-3);
	}

	h2 {
		padding: 0 var(--sp-2);
		margin-bottom: var(--sp-1);
	}

	.node {
		width: 100%;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--sp-3);
		height: 20px;
		background: transparent;
		border-color: transparent;
		text-align: left;
		padding: 0 var(--sp-3);
	}

	.node:hover:not(:disabled) {
		background: var(--surface-alt);
		border-color: transparent;
	}

	.node.active {
		background: var(--accent-soft);
		color: var(--accent);
	}

	.empty {
		padding: var(--sp-5) var(--sp-3);
	}

	footer {
		padding: 0 var(--sp-4);
		height: 20px;
		display: flex;
		align-items: center;
		border-top: 1px solid var(--border);
		flex-shrink: 0;
	}
</style>
