<script lang="ts">
	import { m } from '$lib/paraglide/messages.js';
	import Icon from './Icon.svelte';

	/** A "?" that opens a rich popover on hover or click. Slotted content keeps
	 *  inline `<code>`/`<em>` from the help messages, which a text-only tooltip
	 *  cannot. The card is position:fixed so a parent with overflow:auto (main)
	 *  does not clip it. */
	let {
		label = () => m.common_explanations(),
		children
	}: { label?: () => string; children?: import('svelte').Snippet } = $props();

	let btn = $state<HTMLButtonElement | null>(null);
	let card = $state<HTMLDivElement | null>(null);
	let pinned = $state(false);
	let hovered = $state(false);
	let hideTimer = 0;
	let raf = 0;

	const GAP = 6;
	const MARGIN = 8;

	const open = $derived(hovered || pinned);

	$effect(() => {
		if (!open) return;
		const place = () => position();
		raf = requestAnimationFrame(place);
		window.addEventListener('scroll', place, true);
		window.addEventListener('resize', place);
		return () => {
			cancelAnimationFrame(raf);
			window.removeEventListener('scroll', place, true);
			window.removeEventListener('resize', place);
		};
	});

	function position() {
		if (!btn || !card) return;
		const rect = btn.getBoundingClientRect();
		const cw = card.offsetWidth;
		const ch = card.offsetHeight;
		const vw = document.documentElement.clientWidth;
		const vh = document.documentElement.clientHeight;

		const below = rect.bottom + GAP + ch;
		const above = rect.top - GAP - ch;

		let top: number;
		if (below <= vh) {
			top = rect.bottom + GAP;
		} else if (above >= 0) {
			top = rect.top - GAP - ch;
		} else {
			top = vh - below <= -above ? rect.bottom + GAP : rect.top - GAP - ch;
			top = Math.max(MARGIN, Math.min(top, vh - ch - MARGIN));
		}

		let left = rect.left + rect.width / 2 - cw / 2;
		left = Math.max(MARGIN, Math.min(left, vw - cw - MARGIN));

		card.style.top = `${top}px`;
		card.style.left = `${left}px`;
	}

	function clearHide() {
		clearTimeout(hideTimer);
		hideTimer = 0;
	}

	function scheduleHide() {
		clearHide();
		hideTimer = window.setTimeout(() => (hovered = false), 150);
	}

	function onEnter() {
		clearHide();
		hovered = true;
	}

	function onLeave() {
		scheduleHide();
	}

	function toggle(event: MouseEvent) {
		event.stopPropagation();
		pinned = !pinned;
	}

	// Click outside a pinned popover closes it. The button's own click stops
	// propagation, so this never sees the toggling click.
	function onWinPointerDown(event: PointerEvent) {
		if (!pinned) return;
		const target = event.target as Node | null;
		if (card?.contains(target) || btn?.contains(target)) return;
		pinned = false;
	}
</script>

<svelte:window onpointerdown={onWinPointerDown} />

<span class="info">
	<button
		class="icon-btn"
		class:on={pinned}
		bind:this={btn}
		title={label()}
		aria-label={label()}
		aria-expanded={open}
		onmouseenter={onEnter}
		onmouseleave={onLeave}
		onclick={toggle}
	>
		<Icon name="info" size={13} />
	</button>
	{#if open}
		<div
			class="info-card"
			bind:this={card}
			role="tooltip"
			onmouseenter={onEnter}
			onmouseleave={onLeave}
		>
			<div class="info-body">
				{@render children?.()}
			</div>
		</div>
	{/if}
</span>

<style>
	.info {
		display: inline-flex;
	}
</style>