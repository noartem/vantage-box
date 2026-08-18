<script lang="ts">
	import { icons, type IconName } from '$lib/icons';

	let {
		name,
		size = 14,
		/** Solid fill instead of stroke: at 14px the outline of ▶ and ■ reads worse than a filled shape. */
		fill = false
	}: { name: IconName; size?: number; fill?: boolean } = $props();

	const paths = $derived(icons[name] as readonly string[]);
</script>

<svg
	viewBox="0 0 24 24"
	width={size}
	height={size}
	fill={fill ? 'currentColor' : 'none'}
	stroke={fill ? 'none' : 'currentColor'}
	stroke-width="2"
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
>
	{#each paths as d (d)}
		<path {d} />
	{/each}
</svg>

<style>
	svg {
		display: block;
		flex-shrink: 0;
	}
</style>
