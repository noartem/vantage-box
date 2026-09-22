<script lang="ts">
	import { api, errorText, events } from '$lib/api';
	import GroupCard from '$lib/components/GroupCard.svelte';
	import MiniConnections from '$lib/components/MiniConnections.svelte';
	import MiniLogs from '$lib/components/MiniLogs.svelte';
	import MiniProcess from '$lib/components/MiniProcess.svelte';
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
	{#if error}
		<div class="banner">{error}</div>
	{/if}

	<!-- Top row: service controls (a quarter of the width) next to the in/out
		 traffic charts (three quarters). Both blocks stretch to the same height. -->
	<div class="top">
		<MiniService onopen={() => ongoto('service')} />
		<TrafficChart history={app.trafficHistory} current={app.traffic} />
	</div>

	{#if groups.length > 0}
		<!-- One full-width block: every group is a section inside it, separated
			 by a divider line — a column of small cards wasted the width and
			 hid short groups behind long ones. -->
		<div class="groups card">
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
	</div>

	<!-- The raw process output is a startup log: half the width is enough for
		 it, the rest of the row stays free. -->
	<div class="process-row">
		<MiniProcess {active} />
	</div>
</div>

<style>
	.page {
		display: grid;
		gap: var(--sp-4);
		align-content: start;
	}

	/* Service and traffic chart: a quarter / three quarters on a wide window,
	   half / half in the mid range, and stacked on a narrow one. Both blocks
	   stretch to the same height — the chart card is exactly as tall as the
	   service card. */
	.top {
		display: grid;
		grid-template-columns: minmax(240px, 1fr) minmax(0, 3fr);
		align-items: stretch;
		gap: var(--sp-4);
	}

	/* Mid range: not enough width for a one-quarter service block to keep its
	   form readable, so the two columns go equal. */
	@media (max-width: 1340px) {
		.top {
			grid-template-columns: minmax(240px, 1fr) minmax(0, 1fr);
		}
	}

	@media (max-width: 720px) {
		.top {
			grid-template-columns: 1fr;
		}
	}

	/* Groups flow left to right and wrap, each sized to what it needs: a
		 selector with three nodes does not need a third of the window. The
		 edges stay shared — it is still one block, not several cards. */
	.groups {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-4);
		align-content: flex-start;
	}

	/* The child keeps .section's padding and gap, but its own card chrome is
		 replaced by the shared edges of the wrapper. :global is the only way to
		 reach into a child component's scoped styles. */
	.groups :global(section.section) {
		background: transparent;
		border: none;
		border-radius: 0;
		flex: 1 1 280px;
		max-width: 440px;
		min-width: 0;
	}

	.process-row {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
	}

	@media (max-width: 720px) {
		.process-row {
			grid-template-columns: 1fr;
		}
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
