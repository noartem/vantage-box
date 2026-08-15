<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { app } from '$lib/state.svelte';
	import type { Settings } from '$lib/types';
	import BinaryPanel from '$lib/components/BinaryPanel.svelte';

	let dismissed = $state(false);
	let busy = $state(false);
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

	async function pickConfig() {
		try {
			const path = await api.pickFile('config');
			if (path) await applySettings((s) => (s.singBox.configPath = path));
		} catch (e) {
			error = errorText(e);
		}
	}

	async function createMinimal() {
		try {
			const path = await api.createMinimalConfig();
			await applySettings((s) => (s.singBox.configPath = path));
		} catch (e) {
			error = errorText(e);
		}
	}

	async function pickBinary() {
		try {
			const path = await api.pickFile('binary');
			if (path) await applySettings((s) => (s.singBox.binaryPath = path));
		} catch (e) {
			error = errorText(e);
		}
	}
</script>

{#if dismissed}
	<!-- Чтобы онбординг не терялся после «пропустить»: маленькая кнопка возврата. -->
	<button class="resume" onclick={() => (dismissed = false)} title="Снова показать онбординг">
		Онбординг
	</button>
{:else}
	<div class="overlay">
		<div class="dialog">
			<header>
				<h2>Добро пожаловать в Vantage Box</h2>
				<button class="ghost" onclick={() => (dismissed = true)}>Пропустить</button>
			</header>

			<p class="muted">
				Для работы нужен бинарник sing-box и config.json. Можно выбрать свои или скачать
				sing-box прямо здесь — инсталлер его не включает.
			</p>

			{#if error}
				<div class="banner warn">{error}</div>
			{/if}

			<section class="card step" data-done={binaryOk}>
				<div class="step-head">
					<h3>1. Бинарник sing-box</h3>
					{#if binaryOk}<span class="badge ok">готово</span>{/if}
				</div>
				<p class="muted small">
					Скачайте версию из протестированного диапазона или укажите свой файл sing-box.
				</p>
				<BinaryPanel />
				<div class="actions">
					<button onclick={pickBinary} disabled={busy}>Указать свой файл…</button>
				</div>
			</section>

			<section class="card step" data-done={configOk}>
				<div class="step-head">
					<h3>2. Конфиг sing-box</h3>
					{#if configOk}<span class="badge ok">готово</span>{/if}
				</div>
				<p class="muted small">
					Укажите готовый config.json или создайте минимальный — в нём локальный mixed-инбаунд
					и selector «proxy», куда потом вльются узлы подписок.
				</p>
				<div class="actions">
					<button onclick={pickConfig} disabled={busy}>Указать config.json…</button>
					<button onclick={createMinimal} disabled={busy}>Создать минимальный</button>
				</div>
			</section>

			{#if binaryOk && configOk}
				<div class="banner ok">Готово — можно запускать sing-box на вкладке «Сервис».</div>
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
		padding: 40px 20px;
		overflow-y: auto;
		z-index: 100;
	}

	.dialog {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 12px;
		max-width: 640px;
		width: 100%;
		padding: 20px;
		display: grid;
		gap: 14px;
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}

	h2 {
		font-size: 18px;
		margin: 0;
	}

	h3 {
		font-size: 14px;
		margin: 0;
	}

	.ghost {
		background: transparent;
		border-color: transparent;
		color: var(--text-muted);
	}

	.step {
		display: grid;
		gap: 10px;
	}

	.step-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
	}

	.step[data-done='true'] {
		border-color: var(--good, #2c8);
	}

	.badge.ok {
		color: var(--good, #2c8);
		font-weight: 600;
		font-size: 12px;
	}

	.small {
		font-size: 12px;
		margin: 0;
	}

	.actions {
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
	}

	.resume {
		position: fixed;
		right: 16px;
		bottom: 16px;
		z-index: 100;
		font-size: 12px;
	}
</style>