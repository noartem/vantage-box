<script lang="ts">
	import { formatSpeed } from '$lib/format';
	import type { Traffic } from '$lib/types';

	let { history, current }: { history: Traffic[]; current: Traffic } = $props();

	/** Ширина viewBox; растягиваем по контейнеру через preserveAspectRatio. */
	const W = 100;
	const H = 24;

	// Шкала общая для обеих серий, иначе скачок вниз выглядел бы как скачок вверх.
	const peak = $derived(Math.max(1, ...history.flatMap((point) => [point.up, point.down])));

	/** Линия и область под ней: на 24px высоты один волосок читается хуже пятна. */
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
	const hint = $derived(`Минута истории, общая шкала обеих серий. Пик ${formatSpeed(peak)}`);
</script>

<div class="chart card" title={hint}>
	<div class="series">
		<span class="value mono"><span class="arrow down">↓</span>{formatSpeed(current.down)}</span>
		<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label="Приём">
			{#if down.line}
				<path class="area down" d={down.area} />
				<path class="line down" d={down.line} />
			{/if}
		</svg>
	</div>

	<div class="series">
		<span class="value mono"><span class="arrow up">↑</span>{formatSpeed(current.up)}</span>
		<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label="Отдача">
			{#if up.line}
				<path class="area up" d={up.area} />
				<path class="line up" d={up.line} />
			{/if}
		</svg>
	</div>
</div>

<style>
	/* Полоса, а не карточка-график: текущая скорость и так есть в статус-строке,
	   здесь нужна только форма минуты — под неё хватает 24px. */
	.chart {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--sp-5);
		padding: var(--sp-3) var(--sp-4);
	}

	.series {
		display: grid;
		grid-template-columns: max-content 1fr;
		align-items: center;
		gap: var(--sp-4);
		min-width: 0;
	}

	.value {
		font-size: var(--fs-lg);
		white-space: nowrap;
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

	svg {
		width: 100%;
		height: 24px;
		display: block;
	}

	path.line {
		fill: none;
		stroke-width: 1.5;
		/* Растянутый viewBox иначе деформировал бы толщину линии. */
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

	/* Узкое окно: две колонки превращаются в одну строку с двумя парами. */
	@media (max-width: 640px) {
		.chart {
			grid-template-columns: 1fr;
			gap: var(--sp-2);
		}
	}
</style>
