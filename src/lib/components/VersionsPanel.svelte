<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { formatBytes } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import type { InstallOutcome, ReleaseCatalog, ReleaseInfo } from '$lib/types';
	import Icon from './Icon.svelte';

	const COMPAT_LABELS: Record<string, string> = {
		supported: 'в диапазоне',
		tooNew: 'новее',
		tooOld: 'старее',
		unknown: '—'
	};

	let catalog = $derived(app.catalog);
	let refreshing = $derived(app.catalogRefreshing);
	/** Версия, с которой сейчас идёт работа, и что именно с ней делают. */
	let job = $state<{ version: string; kind: string } | null>(null);

	/** Управлять версиями можно только там, где файл наш. */
	const managed = $derived(app.binaryInfo?.managed === true);

	/** Каталог живёт в общем состоянии и предзагружается при старте приложения,
	 *  поэтому открытие вкладки не мигает загрузкой. Поход на GitHub — только по кнопке. */
	async function loadCatalog(refresh = false) {
		try {
			await app.refreshCatalog(refresh);
		} catch (e) {
			pushAlert('error', errorText(e));
		}
	}

	async function run(version: string, kind: string, call: () => Promise<unknown>) {
		job = { version, kind };
		try {
			const result = await call();
			if (kind === 'use') {
				const outcome = result as InstallOutcome;
				pushAlert(
					'ok',
					`Версия ${outcome.binary.version ?? '—'} теперь используется.${outcome.restarted ? ' sing-box был перезапущен.' : ''}`
				);
				await app.refreshBinaryInfo();
				await loadCatalog();
				await app.refreshRun();
			} else {
				app.catalog = result as ReleaseCatalog;
			}
		} catch (e) {
			pushAlert('error', errorText(e));
			await app.refreshBinaryInfo();
			await loadCatalog();
		} finally {
			job = null;
		}
	}

	function download(release: ReleaseInfo) {
		if (!release.assetUrl) return;
		return run(release.version, 'download', () =>
			api.downloadSingboxRelease(release.version, release.assetUrl as string)
		);
	}

	/** Выбор невыкачанной версии сначала её скачивает — отдельного шага не нужно. */
	async function use(release: ReleaseInfo) {
		if (!release.downloaded) {
			if (!release.assetUrl) return;
			job = { version: release.version, kind: 'download' };
			try {
				app.catalog = await api.downloadSingboxRelease(release.version, release.assetUrl);
			} catch (e) {
				pushAlert('error', errorText(e));
				job = null;
				return;
			}
		}
		await run(release.version, 'use', () => api.useSingboxRelease(release.version));
	}

	function remove(release: ReleaseInfo) {
		return run(release.version, 'delete', () => api.deleteSingboxRelease(release.version));
	}

	const fetchedAt = $derived(
		catalog && catalog.fetchedAt > 0
			? new Date(catalog.fetchedAt * 1000).toLocaleString(undefined, {
					day: '2-digit',
					month: '2-digit',
					hour: '2-digit',
					minute: '2-digit'
				})
			: 'ни разу'
	);
</script>

{#if managed}
	<section class="section">
		<div class="head">
			<h3 class="section-title">Версии</h3>
			<span class="hint">список обновлён: {fetchedAt}</span>
			<span class="spacer"></span>
			<button
				class="icon-btn"
				title="Запросить список с GitHub"
				aria-label="Обновить список"
				disabled={refreshing || job !== null}
				onclick={() => loadCatalog(true)}
			>
				<Icon name="refresh" size={13} />
			</button>
		</div>

		{#if catalog && catalog.releases.length > 0}
			<div class="tbl">
				{#each catalog.releases as release (release.version)}
					<div class="tbl-row" class:active={release.active}>
						<span class="mono">{release.version}</span>

						<span class="chip" data-tone={release.compatibility === 'supported' ? 'good' : undefined}>
							{COMPAT_LABELS[release.compatibility] ?? '—'}
						</span>

						<span class="muted ell">
							{#if release.downloaded}
								на диске
							{:else if release.asset}
								{formatBytes(release.size)}
							{:else}
								нет сборки под эту платформу
							{/if}
						</span>

						{#if release.active}
							<span class="badge">используется</span>
						{:else}
							<button
								disabled={job !== null ||
									(!release.downloaded && !release.assetUrl) ||
									release.compatibility !== 'supported'}
								onclick={() => use(release)}
								title={release.compatibility !== 'supported'
									? 'Автоматически ставим только версии из протестированного диапазона'
									: 'Сделать этой версией рабочего файла'}
							>
								{#if job?.version === release.version && job.kind === 'download'}
									Качаю…
								{:else if job?.version === release.version && job.kind === 'use'}
									Ставлю…
								{:else}
									Выбрать
								{/if}
							</button>
						{/if}

						{#if release.downloaded}
							<button
								class="icon-btn"
								disabled={job !== null || release.active}
								onclick={() => remove(release)}
								title="Удалить скачанный файл этой версии"
								aria-label="Удалить"
							>
								<Icon name="trash" size={12} />
							</button>
						{:else}
							<button
								class="icon-btn"
								disabled={job !== null || !release.assetUrl}
								onclick={() => download(release)}
								title="Скачать, не переключаясь"
								aria-label="Скачать"
							>
								<Icon name="download" size={12} />
							</button>
						{/if}
					</div>
				{/each}
			</div>

			<p class="hint">
				Каждая версия хранится отдельным файлом и остаётся на диске, пока её не удалить: откат не
				требует повторной загрузки. Выбранная версия копируется в рабочий файл, на который
				ссылается служба, — переустанавливать её не нужно.
			</p>
		{:else if catalog}
			<p class="hint">
				Список пуст. Кнопка обновления загрузит его с GitHub, дальше он показывается из кэша.
			</p>
		{:else}
			<p class="hint">Читаю каталог…</p>
		{/if}
	</section>
{/if}

<style>
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
	}

	.tbl-row {
		grid-template-columns: 68px 76px 1fr max-content var(--h-ctl);
		border-radius: var(--radius-ctl);
		border-bottom: none;
	}

	.tbl-row button:not(.icon-btn) {
		height: 18px;
		padding: 0 var(--sp-3);
		font-size: var(--fs-xs);
	}

	.badge {
		color: var(--accent);
		font-size: var(--fs-xs);
		white-space: nowrap;
	}

	.hint {
		max-width: 62ch;
	}
</style>
