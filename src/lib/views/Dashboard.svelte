<script lang="ts">
	import { api, errorText, events } from '$lib/api';
	import GroupCard from '$lib/components/GroupCard.svelte';
	import TrafficChart from '$lib/components/TrafficChart.svelte';
	import { app } from '$lib/state.svelte';
	import type { GroupView } from '$lib/types';

	/** Список групп меняется редко, но выбор могут поменять снаружи — держим свежим. */
	const REFRESH_MS = 5000;

	let groups = $state<GroupView[]>([]);
	let error = $state<string | null>(null);
	let loaded = $state(false);

	async function refresh() {
		try {
			const overview = await api.getProxies();
			groups = overview.groups;
			error = null;
		} catch (e) {
			error = errorText(e);
		} finally {
			loaded = true;
		}
	}

	$effect(() => {
		// Пока связи нет, дёргать API бессмысленно: только копили бы ошибки.
		if (app.status.state !== 'connected') return;

		refresh();
		const timer = setInterval(refresh, REFRESH_MS);
		// Выбор могли поменять из трея или попапа — не ждём следующего опроса.
		const unlisten = events.proxiesChanged(refresh);
		return () => {
			clearInterval(timer);
			unlisten.then((stop) => stop());
		};
	});
</script>

<div class="page">
	<TrafficChart history={app.trafficHistory} current={app.traffic} />

	{#if error}
		<div class="banner">{error}</div>
	{/if}

	{#if groups.length > 0}
		<div class="groups">
			{#each groups as group (group.name)}
				<GroupCard {group} onchanged={refresh} />
			{/each}
		</div>
	{:else if app.status.state !== 'connected'}
		<p class="muted">
			Нет связи с Clash API. Проверьте, что sing-box запущен и в его конфиге включён
			<code>experimental.clash_api</code>.
		</p>
	{:else if loaded}
		<p class="muted">В конфиге sing-box нет групп outbound'ов — переключать нечего.</p>
	{/if}
</div>

<style>
	.page {
		display: grid;
		gap: 16px;
		align-content: start;
	}

	.groups {
		display: grid;
		gap: 12px;
	}

	code {
		font-family: var(--mono);
		font-size: 12px;
		background: var(--surface-alt);
		padding: 1px 5px;
		border-radius: 5px;
	}
</style>
