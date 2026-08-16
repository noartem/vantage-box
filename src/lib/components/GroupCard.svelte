<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { delayTone, formatDelay } from '$lib/format';
	import type { GroupView } from '$lib/types';
	import Icon from './Icon.svelte';

	let {
		group,
		onchanged,
		onjump
	}: {
		group: GroupView;
		/** Просим родителя перечитать /proxies — состояние держит sing-box, не мы. */
		onchanged: () => Promise<void>;
		/** Перейти к карточке вложенной группы. */
		onjump: (name: string) => void;
	} = $props();

	/** Порог, после которого список без поиска перестаёт быть списком. Подписки
	 *  легко приносят полсотни узлов в одну группу. */
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

	/** Перепроверить один узел, не гоняя всю группу: правый клик по строке. */
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
			<span class="chip" title="Выбор внутри этой группы sing-box делает сам">авто</span>
		{/if}
		<span class="spacer"></span>
		<button
			class="icon-btn"
			title={testing ? 'Проверяю задержку…' : 'Проверить задержку всех узлов'}
			aria-label="Проверить задержку"
			disabled={testing}
			onclick={testGroup}
		>
			<Icon name="zap" size={13} />
		</button>
	</header>

	{#if group.now}
		<div class="now ell" title="Текущий узел: {group.now}">{group.now}</div>
	{/if}

	{#if group.items.length >= FILTER_FROM}
		<input
			class="grow"
			type="search"
			placeholder="Фильтр по имени…"
			aria-label="Фильтр узлов"
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
					title="{item.kind}&#10;Правый клик — перепроверить задержку"
				>
					{item.name}
				</button>

				{#if item.udp}
					<span class="chip" title="Узел поддерживает UDP">UDP</span>
				{/if}

				<span class="delay" data-tone={delayTone(item.delay)}>
					{pending === item.name ? '…' : formatDelay(item.delay)}
				</span>

				{#if item.isGroup}
					<button
						class="icon-btn"
						title="Перейти к группе «{item.name}»"
						aria-label="Перейти к группе"
						onclick={() => onjump(item.name)}
					>
						<Icon name="chevronRight" size={12} />
					</button>
				{/if}
			</li>
		{:else}
			<li class="empty hint">Ничего не найдено</li>
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

	/* Текущий узел виден всегда, даже когда список прокручен вниз. */
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
		/* Группа из подписки бывает на полсотни узлов: карточка не должна
		   растягивать всю страницу. */
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
		/* Полоса слева резервируется всегда, иначе строки при выборе съезжали бы. */
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

	/* Кликабельна вся строка: при 22px промахнуться по тексту слишком легко. */
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

	/* Выключенная кнопка в неизменяемой группе — это индикатор, а не «сломано». */
	.node:disabled {
		opacity: 1;
		cursor: default;
	}

	li:not(.active) .node:disabled {
		color: var(--text-muted);
	}
</style>
