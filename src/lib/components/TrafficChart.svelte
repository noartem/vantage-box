<script lang="ts">
	import { formatSpeed } from '$lib/format';
	import type { Traffic } from '$lib/types';

	let { history, current }: { history: Traffic[]; current: Traffic } = $props();

	/** Ширина viewBox; растягиваем по контейнеру через preserveAspectRatio. */
	const W = 100;
	const H = 36;

	// Шкала общая для обеих серий, иначе скачок вниз выглядел бы как скачок вверх.
	const peak = $derived(
		Math.max(1, ...history.flatMap((point) => [point.up, point.down]))
	);

	function line(pick: (point: Traffic) => number): string {
		if (history.length < 2) return '';
		const step = W / (history.length - 1);
		return history
			.map((point, i) => {
				const x = (i * step).toFixed(2);
				const y = (H - (pick(point) / peak) * H).toFixed(2);
				return `${i === 0 ? 'M' : 'L'}${x} ${y}`;
			})
			.join(' ');
	}

	const downPath = $derived(line((p) => p.down));
	const upPath = $derived(line((p) => p.up));
</script>

<div class="chart card">
	<div class="legend">
		<span class="item"><i class="swatch down"></i>вниз {formatSpeed(current.down)}</span>
		<span class="item"><i class="swatch up"></i>вверх {formatSpeed(current.up)}</span>
		<span class="muted peak">пик {formatSpeed(peak)}</span>
	</div>

	<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label="График трафика">
		{#if downPath}
			<path class="down" d={downPath} />
			<path class="up" d={upPath} />
		{/if}
	</svg>

	{#if history.length < 2}
		<p class="muted empty">Ждём данные из /traffic…</p>
	{/if}
</div>

<style>
	.chart {
		padding: 12px 14px;
		display: grid;
		gap: 8px;
	}

	.legend {
		display: flex;
		align-items: center;
		gap: 14px;
		font-variant-numeric: tabular-nums;
	}

	.item {
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}

	.peak {
		margin-left: auto;
	}

	.swatch {
		width: 10px;
		height: 3px;
		border-radius: 2px;
	}

	.swatch.down {
		background: var(--accent);
	}

	.swatch.up {
		background: var(--good);
	}

	svg {
		width: 100%;
		height: 72px;
		display: block;
	}

	svg path {
		fill: none;
		stroke-width: 1.5;
		/* Растянутый viewBox иначе деформировал бы толщину линии. */
		vector-effect: non-scaling-stroke;
		stroke-linejoin: round;
	}

	svg path.down {
		stroke: var(--accent);
	}

	svg path.up {
		stroke: var(--good);
	}

	.empty {
		margin: 0;
	}
</style>
