// How to display a connection. Pure stateless functions: both the full table on
// the tab and the dashboard mini-panel render the same string from one row.

import type { Connection } from './types';

/** Which outbound carries the connection — the last element of the chain. */
export function outbound(c: Connection): string {
	return c.chains.length > 0 ? c.chains[c.chains.length - 1] : '—';
}

/** Connection destination: host if present, otherwise ip:port. */
export function destination(c: Connection): string {
	const m = c.metadata;
	if (m.host) return m.host;
	if (m.destinationIP) return `${m.destinationIP}:${m.destinationPort}`;
	return '—';
}

export function source(c: Connection): string {
	return `${c.metadata.sourceIP}:${c.metadata.sourcePort}`;
}

/** The full path does not fit in the column, and the file name fully answers
 *  "which application opened this". */
export function processName(c: Connection): string {
	const path = c.metadata.processPath;
	if (!path) return '—';
	return path.split(/[\\/]/).pop() || path;
}

export function rule(c: Connection): string {
	return c.rulePayload ? `${c.rule}(${c.rulePayload})` : c.rule;
}

/** Connection age as of `now`: a /connections snapshot does not change `start`,
 *  so time ticks on the caller's side. */
export function age(c: Connection, now: number): number {
	const started = Date.parse(c.start);
	return Number.isNaN(started) ? 0 : now - started;
}
