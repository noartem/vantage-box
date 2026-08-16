<script lang="ts">
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import Icon from './Icon.svelte';
	import { TABS, type TabId } from '$lib/tabs';

	let { tab, ontab }: { tab: TabId; ontab: (next: TabId) => void } = $props();

	/** В обычном браузере (vite dev без Tauri) окна нет — панель просто не
	 *  управляет им, но вкладки продолжают работать. */
	function win() {
		try {
			return getCurrentWindow();
		} catch {
			return null;
		}
	}

	let maximized = $state(false);

	$effect(() => {
		const w = win();
		if (!w) return;
		w.isMaximized().then((value) => (maximized = value));
		// Развернуть могут и мимо наших кнопок: Win+↑, Aero Snap, двойной клик
		// по полосе перетаскивания.
		const unlisten = w.onResized(() => w.isMaximized().then((value) => (maximized = value)));
		return () => {
			unlisten.then((stop) => stop());
		};
	});
</script>

<!-- data-tauri-drag-region висит на самой полосе и на распорке: Tauri проверяет
	 именно целевой элемент события, поэтому клики по кнопкам и вкладкам сюда не
	 попадают, а двойной клик по пустому месту разворачивает окно. -->
<div class="titlebar" data-tauri-drag-region>
	<span class="wordmark" data-tauri-drag-region>Vantage&nbsp;Box</span>

	<nav>
		{#each TABS as item (item.id)}
			<button
				class="tab"
				class:active={tab === item.id}
				title={item.label}
				aria-current={tab === item.id ? 'page' : undefined}
				onclick={() => ontab(item.id)}
			>
				<Icon name={item.icon} size={14} />
				<span class="label">{item.label}</span>
			</button>
		{/each}
	</nav>

	<div class="spacer" data-tauri-drag-region></div>

	<!-- Глифы — из системного шрифта Windows: своя графика тут только разошлась бы
		 с нативной метрикой соседних окон. -->
	<div class="controls">
		<button class="win-btn" aria-label="Свернуть" onclick={() => win()?.minimize()}>&#xE921;</button>
		<button
			class="win-btn"
			aria-label={maximized ? 'Восстановить' : 'Развернуть'}
			onclick={() => win()?.toggleMaximize()}
		>
			{maximized ? '' : ''}
		</button>
		<!-- close(), а не destroy(): окно должно пройти через CloseRequested, иначе
			 настройка «сворачивать в трей» перестанет работать. -->
		<button class="win-btn close" aria-label="Закрыть" onclick={() => win()?.close()}>
			&#xE8BB;
		</button>
	</div>
</div>

<style>
	.titlebar {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: var(--h-title);
		padding-left: var(--sp-4);
		background: var(--surface);
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.wordmark {
		font-size: var(--fs-sm);
		font-weight: 600;
		color: var(--text-muted);
		letter-spacing: 0.02em;
		padding-right: var(--sp-2);
	}

	nav {
		display: flex;
		align-items: center;
		gap: var(--sp-1);
	}

	.tab {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		height: 24px;
		padding: 0 var(--sp-4);
		background: transparent;
		border-color: transparent;
		color: var(--text-muted);
	}

	.tab:hover:not(.active) {
		background: var(--surface-alt);
		border-color: transparent;
		color: var(--text);
	}

	.tab.active {
		background: var(--accent-soft);
		border-color: transparent;
		color: var(--accent);
	}

	.spacer {
		flex: 1;
		align-self: stretch;
		min-width: var(--sp-4);
	}

	.controls {
		display: flex;
		align-self: stretch;
	}

	.win-btn {
		width: var(--w-titlebar-btn);
		height: 100%;
		padding: 0;
		border: none;
		border-radius: 0;
		background: transparent;
		color: var(--text-muted);
		font-family: 'Segoe Fluent Icons', 'Segoe MDL2 Assets', sans-serif;
		font-size: 10px;
		line-height: 1;
	}

	.win-btn:hover:not(:disabled) {
		background: var(--surface-alt);
		border-color: transparent;
		color: var(--text);
	}

	.win-btn.close:hover:not(:disabled) {
		background: #c42b1c;
		color: #fff;
	}

	/* Тесно — подписи уходят в title, остаются иконки: семь вкладок помещаются
	   даже в минимальные 640px. */
	@media (max-width: 1000px) {
		.wordmark {
			display: none;
		}
	}

	@media (max-width: 860px) {
		.label {
			display: none;
		}

		.tab {
			padding: 0 var(--sp-3);
		}
	}
</style>
