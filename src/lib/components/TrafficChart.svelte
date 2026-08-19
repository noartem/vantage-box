<script lang="ts">
	import { formatSpeed } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import type { Traffic } from '$lib/types';

	let { history, current }: { history: Traffic[]; current: Traffic } = $props();

	/** viewBox width; we stretch to the container via preserveAspectRatio. */
	const W = 100;
	const H = 24;

	// Scale is shared by both series, otherwise a downward spike would look like an upward one.
	const peak = $derived(Math.max(1, ...history.flatMap((point) => [point.up, point.down])));

	/** Line and the area beneath it: at 24px height a single hairline reads worse than a filled area. */
	function shape(pick: (point: Traffic) => number): { line: string; area: string } {
		if (history.length < 2) return { line: '', area: '' };
		const step = W / (history.length - 1);
		const line = history
			.map((point, i) => {
				const x = (i * step).toFixed(2);
				const y = (H - (pick(point) / peak) * H).toFixed(2);
				return `${i === 0 ? 'M' : 'L'}${x} ${y}`;
			})
			.join(' ');
		return { line, area: `${line} L${W} ${H} L0 ${H} Z` };
	}

	const down = $derived(shape((p) => p.down));
	const up = $derived(shape((p) => p.up));
	const hint = $derived(m.traffic_hint({ peak: formatSpeed(peak) }));
</script>

<div class="chart card" title={hint}>
	<div class="series">
		<span class="value mono"><span class="arrow down">↓</span>{formatSpeed(current.down)}</span>
		<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label={m.traffic_download()}>
			{#if down.line}
				<path class="area down" d={down.area} />
				<path class="line down" d={down.line} />
			{/if}
		</svg>
	</div>

	<div class="series">
		<span class="value mono"><span class="arrow up">↑</span>{formatSpeed(current.up)}</span>
		<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label={m.traffic_upload()}>
			{#if up.line}
				<path class="area up" d={up.area} />
				<path class="line up" d={up.line} />
			{/if}
		</svg>
	</div>
</div>

<style>
	/* Takes 3/4 of the row's width. No height: the top row uses align-items:
	   stretch, so this card stretches to the service block's height (which
	   sizes the row). A height: 100% would not resolve — the row is auto — and
	   would disable stretch. The min-height is a floor for the stacked
	   (narrow-window) layout, where the chart is in its own row and has no
	   taller sibling to stretch against; the SVG being out of flow, the card
	   would otherwise collapse to zero. */
	.chart {
		display: flex;
		gap: var(--sp-5);
		padding: var(--sp-4);
		min-height: 80px;
	}

	.series {
		position: relative;
		flex: 1 1 0;
		min-width: 0;
	}

	.value {
		position: absolute;
		top: 0;
		left: 0;
		font-size: var(--fs-lg);
		white-space: nowrap;
		padding: 0 var(--sp-2);
		border-radius: 3px;
		/* A faint surface pill so the reading stays legible over the area fill. */
		background: color-mix(in srgb, var(--surface) 72%, transparent);
	}

	.arrow {
		margin-right: var(--sp-2);
	}

	.arrow.down {
		color: var(--accent);
	}

	.arrow.up {
		color: var(--good);
	}

	/* Out of flow so the SVG's intrinsic height (from the viewBox) does not
	   inflate the row: the service block sizes the row, this card stretches to
	   it, and the SVG fills the series via absolute positioning. */
	svg {
		position: absolute;
		top: 0;
		left: 0;
		width: 100%;
		height: 100%;
		display: block;
	}

	path.line {
		fill: none;
		stroke-width: 1.5;
		/* Otherwise the stretched viewBox would deform the line thickness. */
		vector-effect: non-scaling-stroke;
		stroke-linejoin: round;
	}

	path.area {
		stroke: none;
	}

	path.line.down {
		stroke: var(--accent);
	}

	path.area.down {
		fill: color-mix(in srgb, var(--accent) 18%, transparent);
	}

	path.line.up {
		stroke: var(--good);
	}

	path.area.up {
		fill: color-mix(in srgb, var(--good) 18%, transparent);
	}
</style>
