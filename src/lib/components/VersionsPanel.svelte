<script lang="ts">
	import { api, errorText } from '$lib/api';
	import { pushAlert } from '$lib/alerts.svelte';
	import { formatBytes } from '$lib/format';
	import { m } from '$lib/paraglide/messages.js';
	import { app } from '$lib/state.svelte';
	import type { InstallOutcome, ReleaseCatalog, ReleaseInfo } from '$lib/types';
	import Icon from './Icon.svelte';

	/** Compatibility labels — lazy functions: m.x() reads the locale at call time,
	 *  not at module load. Callers invoke COMPAT_LABELS[key](). */
	const COMPAT_LABELS: Record<string, () => string> = {
		supported: () => m.versions_compat_supported(),
		tooNew: () => m.versions_compat_too_new(),
		tooOld: () => m.versions_compat_too_old(),
		unknown: () => m.versions_compat_unknown()
	};

	let catalog = $derived(app.catalog);
	let refreshing = $derived(app.catalogRefreshing);
	/** The version currently being worked on, and what is being done with it. */
	let job = $state<{ version: string; kind: string } | null>(null);

	/** Versions can only be managed where the file is ours. */
	const managed = $derived(app.binaryInfo?.managed === true);

	/** The catalog lives in shared state and is preloaded at app startup, so
	 *  opening the tab does not flash a loader. Hitting GitHub happens only via the button. */
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
				const suffix = outcome.restarted ? ` ${m.versions_restarted_suffix()}` : '';
				pushAlert('ok', `${m.versions_now_used({ version: outcome.binary.version ?? '—' })}${suffix}`);
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

	/** Selecting an undownloaded version first downloads it — no separate step needed. */
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
			: m.versions_never()
	);
</script>

{#if managed}
	<section class="section">
		<div class="head">
			<h3 class="section-title">{m.versions_title()}</h3>
			<span class="hint">{m.versions_list_updated()}: {fetchedAt}</span>
			<span class="spacer"></span>
			<button
				class="icon-btn"
				title={m.versions_refresh_title()}
				aria-label={m.versions_refresh_list()}
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
							{COMPAT_LABELS[release.compatibility]?.() ?? '—'}
						</span>

						<span class="muted ell">
							{#if release.downloaded}
								{m.versions_on_disk()}
							{:else if release.asset}
								{formatBytes(release.size)}
							{:else}
								{m.versions_no_build()}
							{/if}
						</span>

						{#if release.active}
							<span class="badge">{m.versions_in_use()}</span>
						{:else}
							<button
								disabled={job !== null ||
									(!release.downloaded && !release.assetUrl) ||
									release.compatibility !== 'supported'}
								onclick={() => use(release)}
								title={release.compatibility !== 'supported'
									? m.versions_unsupported_title()
									: m.versions_use_title()}
							>
								{#if job?.version === release.version && job.kind === 'download'}
									{m.versions_downloading()}
								{:else if job?.version === release.version && job.kind === 'use'}
									{m.versions_installing()}
								{:else}
									{m.versions_select()}
								{/if}
							</button>
						{/if}

						{#if release.downloaded}
							<button
								class="icon-btn"
								disabled={job !== null || release.active}
								onclick={() => remove(release)}
								title={m.versions_delete_title()}
								aria-label={m.common_delete()}
							>
								<Icon name="trash" size={12} />
							</button>
						{:else}
							<button
								class="icon-btn"
								disabled={job !== null || !release.assetUrl}
								onclick={() => download(release)}
								title={m.versions_download_title()}
								aria-label={m.common_download()}
							>
								<Icon name="download" size={12} />
							</button>
						{/if}
					</div>
				{/each}
			</div>

			<p class="hint">
				{m.versions_hint()}
			</p>
		{:else if catalog}
			<p class="hint">
				{m.versions_empty()}
			</p>
		{:else}
			<p class="hint">{m.versions_reading()}</p>
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
