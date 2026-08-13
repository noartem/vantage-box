<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { delayTone, formatDelay } from '$lib/format';
	import type { GroupView } from '$lib/types';

	let {
		group,
		onchanged
	}: {
		group: GroupView;
		/** Просим родителя перечитать /proxies — состояние держит sing-box, не мы. */
		onchanged: () => Promise<void>;
	} = $props();

	let pending = $state<string | null>(null);
	let testing = $state(false);
	let error = $state<string | null>(null);

	async function select(name: string) {
		if (!group.selectable || name === group.now) return;
		pending = name;
		error = null;
		try {
			await api.selectProxy(group.name, name);
			await onchanged();
		} catch (e) {
			error = errorText(e);
		} finally {
			pending = null;
		}
	}

	async function test() {
		testing = true;
		error = null;
		try {
			await api.testGroupDelay(group.name);
			await onchanged();
		} catch (e) {
			error = errorText(e);
		} finally {
			testing = false;
		}
	}
</script>

<section class="card">
	<header>
		<div class="title">
			<h3>{group.name}</h3>
			<span class="badge">{group.kind}</span>
			{#if !group.selectable}
				<span class="muted" title="Выбор внутри этой группы sing-box делает сам">авто</span>
			{/if}
		</div>
		<button onclick={test} disabled={testing}>
			{testing ? 'Проверяю…' : 'Проверить задержку'}
		</button>
	</header>

	{#if error}
		<div class="banner">{error}</div>
	{/if}

	<ul>
		{#each group.items as item (item.name)}
			<li>
				<button
					class="node"
					class:active={item.name === group.now}
					disabled={!group.selectable || pending !== null}
					onclick={() => select(item.name)}
					title={item.kind}
				>
					<span class="name">{item.name}</span>
					<span class="delay" data-tone={delayTone(item.delay)}>
						{pending === item.name ? '…' : formatDelay(item.delay)}
					</span>
				</button>
			</li>
		{/each}
	</ul>
</section>

<style>
	section {
		padding: 14px;
		display: grid;
		gap: 12px;
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}

	.title {
		display: flex;
		align-items: baseline;
		gap: 8px;
		min-width: 0;
	}

	h3 {
		font-size: 15px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.badge {
		font-size: 11px;
		padding: 2px 6px;
		border-radius: 6px;
		background: var(--surface-alt);
		color: var(--text-muted);
	}

	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
		gap: 8px;
	}

	.node {
		width: 100%;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		text-align: left;
		background: var(--surface-alt);
	}

	.node.active {
		background: var(--accent-soft);
		border-color: var(--accent);
	}

	/* Выключенная кнопка в неизменяемой группе — это индикатор, а не «сломано». */
	.node:disabled {
		opacity: 1;
	}

	.node:disabled:not(.active) {
		color: var(--text-muted);
	}

	.name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.delay {
		font-variant-numeric: tabular-nums;
		font-size: 12px;
		color: var(--text-muted);
		flex-shrink: 0;
	}

	.delay[data-tone='good'] {
		color: var(--good);
	}

	.delay[data-tone='fair'] {
		color: var(--fair);
	}

	.delay[data-tone='poor'] {
		color: var(--poor);
	}
</style>
