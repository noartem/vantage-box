<script lang="ts">
	import { formatTime } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { onopen, active = true }: { onopen: () => void; active?: boolean } = $props();

	/** How many rows we keep in the panel. The card's height is derived from this
	 *  number, so the feed does not stretch the dashboard as logs arrive. */
	const ROWS = 12;

	/** Same importance rank as on the full tab; unknown levels count as important. */
	const RANK: Record<string, number> = { trace: 0, debug: 1, info: 2, warn: 3, error: 4 };
	/** The digest opens quiet, like the journal's default filter: the info
	 *  chatter stays on the full tab, one click away. */
	const MIN_LEVEL = 3;

	// While the tab is inactive, do not recompute the tail: otherwise every log
	// frame would churn the derived for nothing. The block's height is fixed — neighbors do not jump.
	const tail = $derived(
		active
			? app.logs.filter((entry) => (RANK[entry.level.toLowerCase()] ?? 99) >= MIN_LEVEL).slice(-ROWS)
			: []
	);

	/** The panel is narrow: milliseconds would eat a quarter of a row, and they
	 *  are needed when debugging races — i.e. on the full tab. */
	function shortTime(millis: number): string {
		return formatTime(millis).slice(0, 8);
	}
</script>

<section class="section">
	<div class="head">
		<button class="title" title={m.mini_open_logs_tab()} onclick={onopen}>
			<span class="section-title">{m.tabs_logs()}</span>
			<Icon name="external" size={11} />
		</button>

		<span class="muted mono counter">
			{app.logs.length}{app.logsPaused ? ` · ${m.logs_paused_suffix()}` : ''}
		</span>

		<span class="spacer"></span>

		<button
			class="icon-btn"
			class:on={app.logsPaused}
			title={app.logsPaused ? m.logs_resume_title() : m.logs_pause()}
			aria-label={app.logsPaused ? m.common_resume() : m.logs_pause()}
			onclick={() => app.setLogsPaused(!app.logsPaused)}
		>
			<Icon name={app.logsPaused ? 'play' : 'pause'} size={12} fill />
		</button>

		<button
			class="icon-btn"
			title={m.logs_clear_title()}
			aria-label={m.common_clear()}
			disabled={app.logs.length === 0}
			onclick={() => app.clearLogs()}
		>
			<Icon name="trash" size={13} />
		</button>
	</div>

	<!-- Height is always the full panel height: the placeholder sits inside it,
		 so the row of panels never changes shape when data comes and goes. -->
	<div class="feed">
		{#if !active}
			<!-- tab inactive: do not render rows -->
		{:else if tail.length === 0}
			<p class="hint">{m.logs_empty_short()}</p>
		{:else}
			{#each tail as entry (entry.id)}
				<div class="row">
					<span class="time" title={formatTime(entry.time)}>{shortTime(entry.time)}</span>
					<span class="lv" data-level={entry.level.toLowerCase()}>{entry.level}</span>
					<span class="message ell" title={entry.message}>{entry.message}</span>
				</div>
			{/each}
		{/if}
	</div>
</section>

<style>
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	/* The title is a navigation button but should look like a section label. */
	.title {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		height: auto;
		padding: 0;
		background: transparent;
		border: none;
		color: var(--text-muted);
	}

	.title:hover:not(:disabled) {
		border: none;
		color: var(--accent);
	}

	.counter {
		font-size: var(--fs-sm);
		white-space: nowrap;
	}

	/* The feed holds its full height always — the placeholder sits inside it,
	   so the row of panels never changes shape when data comes and goes. */
	.feed {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		height: calc(12 * var(--h-row));
		overflow: hidden;
		font-family: var(--mono);
		font-size: var(--fs-sm);
		user-select: text;
	}

	/* A placeholder centers itself in the fixed-height feed. */
	.hint {
		margin: auto 0;
	}

	.row {
		display: grid;
		grid-template-columns: 58px 36px 1fr;
		gap: var(--sp-3);
		align-items: center;
		height: var(--h-row);
		flex-shrink: 0;
	}

	.time {
		color: var(--text-muted);
	}

	.lv {
		color: var(--text-muted);
		font-size: var(--fs-xs);
		text-transform: uppercase;
	}

	.lv[data-level='warn'] {
		color: var(--fair);
	}

	.lv[data-level='error'] {
		color: var(--poor);
	}
</style>
