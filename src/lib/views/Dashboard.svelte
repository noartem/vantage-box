<script lang="ts">
	import { api, errorText, events } from '$lib/api';
	import GroupCard from '$lib/components/GroupCard.svelte';
	import MiniConnections from '$lib/components/MiniConnections.svelte';
	import MiniLogs from '$lib/components/MiniLogs.svelte';
	import MiniService from '$lib/components/MiniService.svelte';
	import TrafficChart from '$lib/components/TrafficChart.svelte';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { TabId } from '$lib/tabs';
	import type { GroupView } from '$lib/types';

	/** The dashboard shows digests of other tabs, so it can navigate to them —
	 *  +page.svelte owns the switching, as in the alert strip. */
	let { ongoto, active = true }: { ongoto: (tab: TabId) => void; active?: boolean } = $props();

	/** The list of groups changes rarely, but the selection can be changed externally — keep it fresh. */
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

	/** A nested group is a neighboring card on the same page: we do not open
	 *  anything new, just scroll to it. */
	function jump(name: string) {
		document.getElementById(`group-${name}`)?.scrollIntoView({ block: 'nearest' });
	}

	$effect(() => {
		// While there is no connection, polling the API is pointless: we would only accumulate errors.
		if (app.status.state !== 'connected') return;

		refresh();
		const timer = setInterval(refresh, REFRESH_MS);
		// The selection may have been changed from the tray or the popup — do not wait for the next poll.
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
		<!-- Groups flow in columns, not a single stack: in one column a card with
			 three nodes would leave three quarters of the window empty on the right. -->
		<div class="groups">
			{#each groups as group (group.name)}
				<GroupCard {group} onchanged={refresh} onjump={jump} />
			{/each}
		</div>
	{:else if app.status.state !== 'connected'}
		<p class="hint">
			{m.dashboard_no_api()}
			<code class="inline">experimental.clash_api</code>.
		</p>
	{:else if loaded}
		<p class="hint">{m.dashboard_no_groups()}</p>
	{/if}

	<!-- Digests of neighboring tabs: the data for them already flows into shared
		 state regardless of what is open — no separate polling needed. -->
	<div class="minis">
		<MiniConnections {active} onopen={() => ongoto('connections')} />
		<MiniLogs {active} onopen={() => ongoto('logs')} />
		<MiniService onopen={() => ongoto('service')} />
	</div>
</div>

<style>
	.page {
		display: grid;
		gap: var(--sp-4);
		align-content: start;
	}

	.groups {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
		align-items: start;
		gap: var(--sp-4);
	}

	/* 380px, not 300: any narrower and a connection row with host, process and
	   outbound no longer fits. */
	.minis {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
		align-items: start;
		gap: var(--sp-4);
	}
</style>
