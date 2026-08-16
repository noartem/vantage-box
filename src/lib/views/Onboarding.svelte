<script lang="ts">
	import { api, errorText } from '$lib/api';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { app } from '$lib/state.svelte';
	import type { Settings } from '$lib/types';

	let dismissed = $state(false);
	let busy = $state(false);
	// Ошибки онбординга остаются в диалоге: общая строка алертов лежит под
	// оверлеем и оттуда её было бы не видно.
	let error = $state<string | null>(null);

	const binaryOk = $derived(app.binaryInfo?.present === true);
	const configOk = $derived((app.settings?.singBox.configPath.trim() ?? '') !== '');

	/** Применить изменённые настройки: онбординг правит тот же settings.json. */
	async function applySettings(mut: (s: Settings) => void) {
		if (!app.settings) return;
		busy = true;
		error = null;
		try {
			const next = structuredClone($state.snapshot(app.settings)) as Settings;
			mut(next);
			await app.saveSettings(next);
		} catch (e) {
			error = errorText(e);
		} finally {
			busy = false;
		}
	}

	async function guard(action: () => Promise<unknown>) {
		error = null;
		try {
			await action();
		} catch (e) {
			error = errorText(e);
		}
	}

	function pickConfig() {
		return guard(async () => {
			const path = await api.pickFile('config');
			if (path) await applySettings((s) => (s.singBox.configPath = path));
		});
	}

	function createMinimal() {
		return guard(async () => {
			const path = await api.createMinimalConfig();
			await applySettings((s) => (s.singBox.configPath = path));
		});
	}

	function pickBinary() {
		return guard(async () => {
			const path = await api.pickFile('binary');
			if (path) await applySettings((s) => (s.singBox.binaryPath = path));
		});
	}
</script>

{#if dismissed}
	<!-- Чтобы онбординг не терялся после «пропустить»: маленькая кнопка возврата. -->
	<button class="resume" onclick={() => (dismissed = false)} title="Снова показать онбординг">
		Онбординг
	</button>
{:else}
	<div class="overlay">
		<div class="dialog bounce">
			<header>
				<h2>Добро пожаловать в Vantage Box</h2>
				<span class="spacer"></span>
				<button class="ghost" onclick={() => (dismissed = true)}>Пропустить</button>
			</header>

			<p class="hint">
				Нужны бинарник sing-box и config.json. Можно выбрать свои или скачать sing-box прямо
				здесь — инсталлер его не включает.
			</p>

			{#if error}
				<div class="banner warn">{error}</div>
			{/if}

			<section class="section step" data-done={binaryOk}>
				<div class="step-head">
					<h3 class="section-title">1. Бинарник sing-box</h3>
					<span class="spacer"></span>
					{#if binaryOk}
						<span class="chip" data-tone="good">
							<Icon name="check" size={10} />
							готово
						</span>
					{/if}
				</div>

				<BinaryPanel />

				<div class="toolbar">
					<button onclick={pickBinary} disabled={busy}>Указать свой файл…</button>
				</div>
			</section>

			<section class="section step" data-done={configOk}>
				<div class="step-head">
					<h3 class="section-title">2. Конфиг sing-box</h3>
					<span class="spacer"></span>
					{#if configOk}
						<span class="chip" data-tone="good">
							<Icon name="check" size={10} />
							готово
						</span>
					{/if}
				</div>

				<p class="hint">
					Готовый config.json или минимальный: локальный mixed-инбаунд и selector «proxy», куда
					потом вльются узлы подписок.
				</p>

				<div class="toolbar">
					<button onclick={pickConfig} disabled={busy}>Указать config.json…</button>
					<button onclick={createMinimal} disabled={busy}>Создать минимальный</button>
				</div>
			</section>

			{#if binaryOk && configOk}
				<div class="banner ok">Готово — можно запускать sing-box кнопкой в статус-строке.</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: flex-start;
		justify-content: center;
		padding: var(--sp-6);
		z-index: 100;
	}

	/* Диалог прокручивается сам: вместе с вложенным каталогом версий он легко
	   перерастал высоту окна. */
	.dialog {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius-card);
		max-width: 620px;
		width: 100%;
		max-height: 100%;
		overflow-y: auto;
		padding: var(--sp-5);
		display: grid;
		gap: var(--sp-4);
		align-content: start;
	}

	header {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	h2 {
		font-size: var(--fs-lg);
	}

	.ghost {
		background: transparent;
		border-color: transparent;
		color: var(--text-muted);
	}

	.step-head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	.step[data-done='true'] {
		border-color: var(--good);
	}

	.chip {
		gap: var(--sp-1);
	}

	.resume {
		position: fixed;
		right: var(--sp-5);
		bottom: calc(var(--h-status) + var(--sp-3));
		z-index: 100;
	}
</style>
