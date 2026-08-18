/**
 * Tooltip popup that portals itself into `<body>` and positions itself so it fits
 * in the window. The native `title` is not shown on disabled buttons in WebView2,
 * and an in-markup balloon would be clipped by a parent with `overflow:auto`
 * (this is how `main` clipped it past the top of the window).
 *
 * Usage:
 *   <span use:tooltip={text ? '…' : ''}>…</span>
 *
 * Empty text — no tooltip: no node is created, hover is not blocked.
 * Position is computed by `position()`: by default below the anchor, and if there
 * is not enough room — above; horizontally it is clamped into the window.
 */
export function tooltip(node: HTMLElement, text: string) {
	let card: HTMLDivElement | null = null;
	let current = text;
	let raf = 0;
	let hideTimer = 0;

	const GAP = 6;
	const MARGIN = 8;
	const FADE_MS = 120;

	function show() {
		if (!current || card) return;
		card = document.createElement('div');
		card.className = 'tip-balloon';
		card.textContent = current;
		// Start with opacity:0 so positioning does not flash.
		card.style.opacity = '0';
		document.body.appendChild(card);
		position();
		raf = requestAnimationFrame(() => {
			card?.style.removeProperty('opacity');
		});
	}

	function hide() {
		cancelAnimationFrame(raf);
		if (!card) return;
		const el = card;
		card = null;
		el.style.opacity = '0';
		const remove = () => el.remove();
		el.addEventListener('transitionend', remove, { once: true });
		clearTimeout(hideTimer);
		hideTimer = window.setTimeout(remove, FADE_MS + 40);
	}

	function position() {
		if (!card) return;
		const rect = node.getBoundingClientRect();
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
			// Fits neither above nor below — go where there is more room, clamped into the window.
			top = vh - below <= -above ? rect.bottom + GAP : rect.top - GAP - ch;
			top = Math.max(MARGIN, Math.min(top, vh - ch - MARGIN));
		}

		let left = rect.left + rect.width / 2 - cw / 2;
		left = Math.max(MARGIN, Math.min(left, vw - cw - MARGIN));

		card.style.top = `${top}px`;
		card.style.left = `${left}px`;
	}

	const onEnter = () => show();
	const onLeave = () => hide();

	node.addEventListener('mouseenter', onEnter);
	node.addEventListener('mouseleave', onLeave);
	// Parent scroll moves the anchor — reposition (or hide if it has gone away).
	window.addEventListener('scroll', position, true);
	window.addEventListener('resize', position);

	return {
		update(next: string) {
			current = next;
			if (card) {
				if (!current) hide();
				else {
					card.textContent = current;
					position();
				}
			}
		},
		destroy() {
			hide();
			node.removeEventListener('mouseenter', onEnter);
			node.removeEventListener('mouseleave', onLeave);
			window.removeEventListener('scroll', position, true);
			window.removeEventListener('resize', position);
		}
	};
}