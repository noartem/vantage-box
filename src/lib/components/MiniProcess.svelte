<script lang="ts">
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { active = true }: { active?: boolean } = $props();

	/** How many rows we keep in the panel. The card's height is derived from this
	 *  number, so the feed does not stretch the dashboard as lines arrive. */
	const ROWS = 12;

	// While the tab is inactive, do not recompute the tail: otherwise every chunk
	// would churn the derived for nothing. The block's height is held by .filled — neighbors do not jump.
	const tail = $derived(active ? app.coreLogs.slice(-ROWS) : []);

	/** Startup failures must stand out at a glance; sing-box marks its lines
	 *  with FATAL/ERROR/PANIC levels, raw output has nothing else to go by. */
	const BAD = /\b(FATAL|ERROR|PANIC)\b/;
</script>

<section class="section">
	<div class="head">
		<span class="section-title">{m.mini_process_title()}</span>

		<span class="muted mono counter">
			{app.coreLogs.length}{app.coreLogsPaused ? ` · ${m.logs_paused_suffix()}` : ''}
		</span>

		<span class="spacer"></span>

		<button
			class="icon-btn"
			class:on={app.coreLogsPaused}
			title={app.coreLogsPaused ? m.logs_resume_title() : m.logs_pause()}
			aria-label={app.coreLogsPaused ? m.common_resume() : m.logs_pause()}
			onclick={() => app.setCoreLogsPaused(!app.coreLogsPaused)}
		>
			<Icon name={app.coreLogsPaused ? 'play' : 'pause'} size={12} fill />
		</button>

		<button
			class="icon-btn"
			title={m.logs_clear_title()}
			aria-label={m.common_clear()}
			disabled={app.coreLogs.length === 0}
			onclick={() => app.clearCoreLogs()}
		>
			<Icon name="trash" size={13} />
		</button>
	</div>

	<!-- Height is fixed only when there is something to show: an empty feed
		 should not hold twelve rows of whitespace. filled depends on the presence
		 of data, not on active — a hidden block keeps the same height as a visible one. -->
	<div class="feed" class:filled={app.coreLogs.length > 0}>
		{#if !active}
			<!-- tab inactive: do not render rows, height is held by .filled -->
		{:else if tail.length === 0}
			<p class="hint">{m.mini_process_empty()}</p>
		{:else}
			{#each tail as line, i (i)}
				<div class="row" class:bad={BAD.test(line)}>
					<span class="message ell" title={line}>{line}</span>
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

	.counter {
		font-size: var(--fs-sm);
		white-space: nowrap;
	}

	.feed {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		overflow: hidden;
		font-family: var(--mono);
		font-size: var(--fs-sm);
		user-select: text;
	}

	/* Rows are pinned to the bottom of a fixed-height window: while there are
	   fewer than ROWS, the newest one still lands where expected, and the panel
	   does not grow with each new line, jostling its row neighbors. */
	.feed.filled {
		height: calc(12 * var(--h-row));
	}

	.row {
		display: flex;
		align-items: center;
		height: var(--h-row);
		flex-shrink: 0;
	}

	/* Raw process lines have no API timestamps — the whole row is the message. */
	.message {
		min-width: 0;
	}

	.row.bad .message {
		color: var(--poor);
	}
</style>
