// sing-box control: one wrapper for every place that starts it — the Service tab,
// the dashboard mini-panel, and the status bar. Each used to keep its own copy of
// try/catch/refreshRun.

import { m } from '$lib/paraglide/messages.js';
import { errorText } from './api';
import { pushAlert } from './alerts.svelte';
import { app } from './state.svelte';
import type { RestartOutcome, ServiceState } from './types';

/** Labels are lazy functions: m.x() reads the locale at call time (render), not
 *  at module load. Callers render with `SERVICE_LABELS[state]()`. */
export const SERVICE_LABELS: Record<ServiceState, () => string> = {
	notInstalled: () => m.service_state_not_installed(),
	stopped: () => m.service_state_stopped(),
	startPending: () => m.service_state_start_pending(),
	running: () => m.service_state_running(),
	stopPending: () => m.service_state_stop_pending(),
	unknown: () => m.service_state_unknown()
};

/** The restart outcome is an event, not a state: it belongs in the alert strip,
 *  not in a banner nobody later dismisses. */
export function reportRestart(outcome: RestartOutcome) {
	const skipped =
		outcome.skipped.length > 0
			? ` ${m.service_restart_skipped({ items: outcome.skipped.join('; ') })}`
			: '';
	if (!outcome.apiBack) {
		pushAlert('warn', m.service_restart_api_down({ skipped }));
		return;
	}
	const restored =
		outcome.restored.length > 0
			? m.service_restart_restored({ items: outcome.restored.join(', ') })
			: m.service_restart_no_restore();
	pushAlert('ok', m.service_restart_done({ restored, skipped }));
}

/** Runs an action on sing-box and tidies up state. An action error is a one-off
 *  event: it goes to the alert strip, not a second banner growing the calling
 *  panel. */
export async function runServiceAction(kind: string, call: () => Promise<unknown>): Promise<void> {
	try {
		const result = await call();
		if (kind === 'restart') reportRestart(result as RestartOutcome);
	} catch (e) {
		pushAlert('error', errorText(e));
	} finally {
		await app.refreshRun();
	}
}