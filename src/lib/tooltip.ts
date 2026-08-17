/**
 * Попап-подсказка, которая сама уходит в `<body>` и позиционируется так, чтобы
 * влезть в окно. Нативный `title` на disabled-кнопке в WebView2 не показывается,
 * а балун внутри разметки clipped бы родителем с `overflow:auto` (так `main`
 * обрезал его за верхом окна).
 *
 * Использование:
 *   <span use:tooltip={text ? '…' : ''}>…</span>
 *
 * Пустой текст — подсказки нет: узел не создаётся, hover не блокируется.
 * Позицию считает `position()`: по умолчанию снизу от якоря, не хватило места —
 * сверху; по горизонтали прижимается в окно.
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
		// Сначала opacity:0, чтобы позиционирование не мигнуло.
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
			// Не помещается ни сверху, ни снизу — туда, где места больше, и в окно.
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
	// Скролл родителя двигает якорь — перепозиционируем (или прячем, если ушёл).
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