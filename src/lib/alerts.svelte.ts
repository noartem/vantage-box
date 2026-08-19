// One-off action-error messages. Persistent problems (broken settings.json,
// unreachable API, incompatible version) are not surfaced from here: they come
// from the `app` state straight into AlertStrip and disappear on their own once
// the cause goes away. This holds only things that have no state of their own —
// for example, "failed to stop sing-box".

export type AlertSeverity = 'error' | 'warn' | 'ok';

export type AlertAction = {
	label: string;
	run: () => void;
};

export type TransientAlert = {
	id: number;
	severity: AlertSeverity;
	text: string;
	/** Optional action button (e.g. "Running config" on a startup error).
	 *  When set, the alert strip renders it instead of the "Hide" button and
	 *  keeps a separate dismiss ✕. */
	action?: AlertAction;
};

class Transient {
	items = $state<TransientAlert[]>([]);
	private nextId = 1;

	push(severity: AlertSeverity, text: string, action?: AlertAction): number {
		const id = this.nextId++;
		// Do not stack the same text repeatedly: three identical lines in a "1/3"
		// counter is noise, not information.
		this.items = [...this.items.filter((a) => a.text !== text), { id, severity, text, action }];
		return id;
	}

	dismiss(id: number) {
		this.items = this.items.filter((a) => a.id !== id);
	}

	clear() {
		this.items = [];
	}
}

export const transientAlerts = new Transient();

export function pushAlert(severity: AlertSeverity, text: string, action?: AlertAction): number {
	return transientAlerts.push(severity, text, action);
}

export function dismissAlert(id: number) {
	transientAlerts.dismiss(id);
}
