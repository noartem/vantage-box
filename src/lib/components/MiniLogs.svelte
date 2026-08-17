<script lang="ts">
	import { formatTime } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import Icon from './Icon.svelte';

	let { onopen, active = true }: { onopen: () => void; active?: boolean } = $props();

	/** Сколько строк держим в панели. Высота карточки от этого числа и считается,
	 *  поэтому лента не растягивает дашборд по мере поступления логов. */
	const ROWS = 12;

	// Пока вкладка не активна, хвост не пересчитываем: иначе каждый лог-кадр
	// дёргал бы derived впустую. Высоту блока держит .filled — соседи не прыгают.
	const tail = $derived(active ? app.logs.slice(-ROWS) : []);

	/** Панель узкая: миллисекунды съедали бы четверть строки, а нужны они при
	 *  разборе гонок — то есть на полной вкладке. */
	function shortTime(millis: number): string {
		return formatTime(millis).slice(0, 8);
	}
</script>

<section class="section">
	<div class="head">
		<button class="title" title="Открыть вкладку «Логи»" onclick={onopen}>
			<span class="section-title">Логи</span>
			<Icon name="external" size={11} />
		</button>

		<span class="muted mono counter">
			{app.logs.length}{app.logsPaused ? ' · пауза' : ''}
		</span>

		<span class="spacer"></span>

		<button
			class="icon-btn"
			class:on={app.logsPaused}
			title={app.logsPaused ? 'Продолжить: накопленное появится сразу' : 'Пауза'}
			aria-label={app.logsPaused ? 'Продолжить' : 'Пауза'}
			onclick={() => app.setLogsPaused(!app.logsPaused)}
		>
			<Icon name={app.logsPaused ? 'play' : 'pause'} size={12} fill />
		</button>

		<button
			class="icon-btn"
			title="Очистить ленту"
			aria-label="Очистить"
			disabled={app.logs.length === 0}
			onclick={() => app.clearLogs()}
		>
			<Icon name="trash" size={13} />
		</button>
	</div>

	<!-- Высота фиксируется только когда есть что показывать: пустая лента не
		 должна держать двенадцать строк белого места. filled зависит от наличия
		 данных, а не от active — скрытый блок держит ту же высоту, что и видимый. -->
	<div class="feed" class:filled={app.logs.length > 0}>
		{#if !active}
			<!-- вкладка не активна: строки не рисуем, высоту держит .filled -->
		{:else if tail.length === 0}
			<p class="hint">Логи ещё не приходили.</p>
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

	/* Заголовок — кнопка перехода, но выглядеть должен подписью секции. */
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

	.feed {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		overflow: hidden;
		font-family: var(--mono);
		font-size: var(--fs-sm);
		user-select: text;
	}

	/* Строки прижаты к низу окна постоянной высоты: пока их меньше ROWS, свежая
	   всё равно оказывается там, где её ждут, и панель не растёт с каждой новой
	   записью, дёргая соседей по ряду. */
	.feed.filled {
		height: calc(12 * var(--h-row));
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
