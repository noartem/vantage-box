<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { formatBytes } from '$lib/format';
	import { app } from '$lib/state.svelte';
	import type { BinaryInfo, InstallOutcome, ReleaseCatalog, ReleaseInfo } from '$lib/types';

	let info = $state<BinaryInfo | null>(null);
	let catalog = $state<ReleaseCatalog | null>(null);
	let outcome = $state<InstallOutcome | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(false);
	let refreshing = $state(false);
	/** Версия, с которой сейчас идёт работа, и что именно с ней делают. */
	let job = $state<{ version: string; kind: string } | null>(null);

	async function refreshInfo() {
		loading = true;
		try {
			info = await api.getBinaryInfo();
			error = null;
		} catch (e) {
			error = errorText(e);
		} finally {
			loading = false;
		}
	}

	/** Каталог всегда приезжает из кэша: поход на GitHub — только по кнопке. */
	async function loadCatalog(refresh = false) {
		if (refresh) refreshing = true;
		error = null;
		try {
			catalog = await api.listSingboxReleases(refresh);
		} catch (e) {
			error = errorText(e);
		} finally {
			refreshing = false;
		}
	}

	async function run(version: string, kind: string, call: () => Promise<unknown>) {
		job = { version, kind };
		error = null;
		outcome = null;
		try {
			const result = await call();
			if (kind === 'use') {
				outcome = result as InstallOutcome;
				info = outcome.binary;
				await loadCatalog();
				await app.refreshRun();
			} else {
				catalog = result as ReleaseCatalog;
			}
		} catch (e) {
			error = errorText(e);
			await refreshInfo();
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
				catalog = await api.downloadSingboxRelease(release.version, release.assetUrl);
			} catch (e) {
				error = errorText(e);
				job = null;
				return;
			}
		}
		await run(release.version, 'use', () => api.useSingboxRelease(release.version));
	}

	function remove(release: ReleaseInfo) {
		return run(release.version, 'delete', () => api.deleteSingboxRelease(release.version));
	}

	$effect(() => {
		// Перечитываем при смене настроек: путь к файлу sing-box мог поменяться.
		app.settings?.singBox.binaryPath;
		refreshInfo();
		loadCatalog();
	});

	/** Управлять версиями можно только там, где файл наш. */
	const managed = $derived(info?.managed === true);
	const fetchedAt = $derived(
		catalog && catalog.fetchedAt > 0
			? new Date(catalog.fetchedAt * 1000).toLocaleString()
			: 'ни разу'
	);
</script>

<section class="card">
	<header>
		<h3>Файл sing-box</h3>
		<button onclick={refreshInfo} disabled={loading}>
			{loading ? 'Проверяю…' : 'Обновить данные'}
		</button>
	</header>

	{#if error}
		<div class="banner">{error}</div>
	{/if}

	{#if info}
		<dl>
			<dt>путь</dt>
			<dd class="selectable">{info.path}</dd>
			<dt>режим</dt>
			<dd>{info.managed ? 'под управлением Vantage Box' : 'задан вручную'}</dd>
			<dt>версия</dt>
			<dd>{info.version ?? (info.present ? 'не определена' : 'нет файла')}</dd>
			<dt>поддерживается</dt>
			<dd>{info.supportedRange}</dd>
		</dl>

		{#if info.problem}
			<div class="banner">{info.problem}</div>
		{/if}

		{#if info.compatibility === 'tooOld'}
			<div class="banner warn">Версия ниже поддерживаемого диапазона — обновите её.</div>
		{:else if info.compatibility === 'tooNew'}
			<div class="banner warn">
				Версия выше протестированного диапазона. Работать будет, но поведение может отличаться.
			</div>
		{/if}

		{#if !managed}
			<p class="muted hint">
				Путь задан вручную, поэтому Vantage Box этот файл не трогает — только сообщает о
				несовместимой версии. Очистите поле «Файл sing-box» в настройках, чтобы отдать версии
				приложению.
			</p>
		{/if}
	{/if}

	{#if outcome}
		<div class="banner ok">
			<strong>Версия {outcome.binary.version ?? '—'} теперь используется.</strong>
			{#if outcome.restarted}sing-box был перезапущен.{/if}
		</div>
	{/if}
</section>

{#if managed}
	<section class="card">
		<header>
			<div class="title">
				<h3>Версии</h3>
				<span class="muted stamp">список обновлён: {fetchedAt}</span>
			</div>
			<button onclick={() => loadCatalog(true)} disabled={refreshing || job !== null}>
				{refreshing ? 'Запрашиваю…' : 'Обновить список'}
			</button>
		</header>

		{#if catalog && catalog.releases.length > 0}
			<ul>
				{#each catalog.releases as release (release.version)}
					<li class:active={release.active}>
						<span class="version">{release.version}</span>
						<span class="tag" data-compat={release.compatibility}>
							{release.compatibility === 'supported'
								? 'в диапазоне'
								: release.compatibility === 'tooNew'
									? 'новее диапазона'
									: release.compatibility === 'tooOld'
										? 'старее диапазона'
										: '—'}
						</span>
						<span class="muted size">
							{#if release.downloaded}
								скачана
							{:else if release.asset}
								{formatBytes(release.size)}
							{:else}
								нет сборки под эту платформу
							{/if}
						</span>

						<span class="row-actions">
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
										: undefined}
								>
									{#if job?.version === release.version && job.kind === 'download'}
										Качаю…
									{:else if job?.version === release.version && job.kind === 'use'}
										Переключаю…
									{:else}
										Выбрать
									{/if}
								</button>
							{/if}

							{#if release.downloaded}
								<button
									disabled={job !== null || release.active}
									onclick={() => remove(release)}
									title="Удалить скачанный файл этой версии"
								>
									{job?.version === release.version && job.kind === 'delete' ? 'Удаляю…' : 'Удалить'}
								</button>
							{:else}
								<button
									disabled={job !== null || !release.assetUrl}
									onclick={() => download(release)}
								>
									{job?.version === release.version && job.kind === 'download'
										? 'Качаю…'
										: 'Скачать'}
								</button>
							{/if}
						</span>
					</li>
				{/each}
			</ul>

			<p class="muted hint">
				Каждая версия хранится отдельным файлом и остаётся на диске, пока её не удалить: откат на
				предыдущую не требует повторной загрузки. Выбранная версия копируется в рабочий файл, на
				который ссылается сервис, — переустанавливать его не нужно.
			</p>
		{:else if catalog}
			<p class="muted">
				Список пуст. Нажмите «Обновить список» — он загрузится с GitHub и дальше будет показываться
				из кэша.
			</p>
		{:else}
			<p class="muted">Читаю каталог…</p>
		{/if}
	</section>
{/if}

<style>
	section {
		padding: 14px;
		display: grid;
		gap: 10px;
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}

	.title {
		display: flex;
		align-items: baseline;
		gap: 10px;
	}

	.stamp {
		font-size: 12px;
	}

	h3 {
		font-size: 14px;
	}

	dl {
		margin: 0;
		display: grid;
		grid-template-columns: 170px 1fr;
		gap: 4px 10px;
		font-size: 12px;
	}

	dt {
		color: var(--text-muted);
	}

	dd {
		margin: 0;
		font-family: var(--mono);
		word-break: break-all;
	}

	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: 4px;
	}

	li {
		display: grid;
		grid-template-columns: 90px 130px 1fr auto;
		align-items: center;
		gap: 10px;
		font-size: 12px;
		padding: 4px 6px;
		border-radius: 8px;
	}

	li.active {
		background: var(--accent-soft);
	}

	.row-actions {
		display: flex;
		gap: 6px;
		justify-content: flex-end;
	}

	.badge {
		color: var(--accent);
		font-weight: 600;
	}

	.version {
		font-family: var(--mono);
	}

	.tag {
		color: var(--text-muted);
	}

	.tag[data-compat='supported'] {
		color: var(--good);
	}

	.tag[data-compat='tooNew'],
	.tag[data-compat='tooOld'] {
		color: var(--fair);
	}

	.hint {
		margin: 0;
		font-size: 12px;
	}
</style>
