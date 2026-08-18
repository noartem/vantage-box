<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { delayTone, formatDelay } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import type { GroupView } from '$lib/types';
	import Icon from './Icon.svelte';

	let {
		group,
		onchanged,
		onjump
	}: {
		group: GroupView;
		/** Ask the parent to re-read /proxies — sing-box holds the state, not us. */
		onchanged: () => Promise<void>;
		/** Jump to a nested group's card. */
		onjump: (name: string) => void;
	} = $props();

	/** Threshold past which a list without search stops being a list. Subscriptions
	 *  easily bring half a hundred nodes into a single group. */
	const FILTER_FROM = 12;

	let pending = $state<string | null>(null);
	let testing = $state(false);
	let filter = $state('');

	const items = $derived(
		filter.trim() === ''
			? group.items
			: group.items.filter((item) => item.name.toLowerCase().includes(filter.trim().toLowerCase()))
	);

	async function select(name: string) {
		if (!group.selectable || name === group.now) return;
		pending = name;
		try {
			await api.selectProxy(group.name, name);
			await onchanged();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			pending = null;
		}
	}

	async function testGroup() {
		testing = true;
		try {
			await api.testGroupDelay(group.name);
			await onchanged();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			testing = false;
		}
	}

	/** Re-test a single node without running the whole group: right-click on a row. */
	async function testOne(event: MouseEvent, name: string) {
		event.preventDefault();
		if (pending !== null) return;
		pending = name;
		try {
			await api.testProxyDelay(name);
			await onchanged();
		} catch (e) {
			pushAlert('error', errorText(e));
		} finally {
			pending = null;
		}
	}
</script>

<section class="section" id="group-{group.name}">
	<header>
		<h3 class="ell" title={group.name}>{group.name}</h3>
		<span class="chip">{group.kind}</span>
		{#if !group.selectable}
			<span class="chip" title={m.group_auto_title()}>{m.group_auto()}</span>
		{/if}
		<span class="spacer"></span>
		<button
			class="icon-btn"
			title={testing ? m.group_testing_title() : m.group_test_title()}
			aria-label={m.group_test_label()}
			disabled={testing}
			onclick={testGroup}
		>
			<Icon name="zap" size={13} />
		</button>
	</header>

	{#if group.now}
		<div class="now ell" title={m.group_now_label({ node: group.now })}>{group.now}</div>
	{/if}

	{#if group.items.length >= FILTER_FROM}
		<input
			class="grow"
			type="search"
			placeholder={m.group_filter_placeholder()}
			aria-label={m.group_filter_label()}
			bind:value={filter}
		/>
	{/if}

	<ul class="bounce">
		{#each items as item (item.name)}
			<li class:active={item.name === group.now}>
				<button
					class="node ell"
					disabled={!group.selectable || pending !== null}
					onclick={() => select(item.name)}
					oncontextmenu={(event) => testOne(event, item.name)}
					title={`${item.kind}\n${m.group_retest_hint()}`}
				>
					{item.name}
				</button>

				{#if item.udp}
					<span class="chip" title={m.group_udp_title()}>UDP</span>
				{/if}

				<span class="delay" data-tone={delayTone(item.delay)}>
					{pending === item.name ? '…' : formatDelay(item.delay)}
				</span>

				{#if item.isGroup}
					<button
						class="icon-btn"
						title={m.group_goto_group_title({ name: item.name })}
						aria-label={m.group_goto_group_label()}
						onclick={() => onjump(item.name)}
					>
						<Icon name="chevronRight" size={12} />
					</button>
				{/if}
			</li>
		{:else}
			<li class="empty hint">{m.group_nothing_found()}</li>
		{/each}
	</ul>
</section>

<style>
	header {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		min-width: 0;
	}

	h3 {
		font-size: var(--fs-md);
		font-weight: 600;
		text-transform: none;
		letter-spacing: 0;
		color: var(--text);
		min-width: 0;
	}

	/* The current node is always visible, even when the list is scrolled down. */
	.now {
		font-size: var(--fs-sm);
		color: var(--accent);
	}

	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		align-content: start;
		/* A subscription group can have half a hundred nodes: the card must not
		   stretch the whole page. */
		max-height: 264px;
		overflow-y: auto;
	}

	li {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		height: var(--h-row);
		padding-right: var(--sp-2);
		border-radius: var(--radius-ctl);
		/* The left band is always reserved, otherwise rows would shift on selection. */
		border-left: 2px solid transparent;
	}

	li:hover {
		background: var(--surface-alt);
	}

	li.active {
		background: var(--accent-soft);
		border-left-color: var(--accent);
	}

	li.empty {
		justify-content: center;
	}

	/* The whole row is clickable: at 22px it is too easy to miss the text. */
	.node {
		flex: 1;
		min-width: 0;
		height: 100%;
		text-align: left;
		padding: 0 var(--sp-3);
		background: transparent;
		border-color: transparent;
	}

	.node:hover:not(:disabled) {
		border-color: transparent;
	}

	/* A disabled button in a non-editable group is an indicator, not "broken". */
	.node:disabled {
		opacity: 1;
		cursor: default;
	}

	li:not(.active) .node:disabled {
		color: var(--text-muted);
	}
</style>
