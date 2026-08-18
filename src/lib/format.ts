import { m } from '$lib/paraglide/messages.js';

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** Human-readable size: 0 B, 812 B, 1.4 MB. */
export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < UNITS.length - 1) {
		value /= 1024;
		unit += 1;
	}
	// Bytes are integers, everything else gets one decimal: the column of digits does not "jump".
	return `${unit === 0 ? Math.round(value) : value.toFixed(1)} ${UNITS[unit]}`;
}

/** Clash reports traffic as "over the last second", so this is already a speed. */
export function formatSpeed(bytesPerSecond: number): string {
	return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatTime(millis: number): string {
	const d = new Date(millis);
	const pad = (n: number, width = 2) => String(n).padStart(width, '0');
	return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

/** Connection age. The column is narrow, so the unit is always a single one. */
export function formatDuration(millis: number): string {
	if (!Number.isFinite(millis) || millis < 0) return '—';
	const seconds = Math.floor(millis / 1000);
	if (seconds < 60) return m.format_duration_seconds({ n: seconds });
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return m.format_duration_minutes({ n: minutes });
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return m.format_duration_hours({ n: hours });
	return m.format_duration_days({ n: Math.floor(hours / 24) });
}

/** Node delay: null — not measured, 0 or below — the node did not respond. */
export function formatDelay(delay: number | null): string {
	if (delay === null || delay <= 0) return '—';
	return `${delay} ms`;
}

/** Threshold for color-coded delay. */
export function delayTone(delay: number | null): 'none' | 'good' | 'fair' | 'poor' {
	if (delay === null || delay <= 0) return 'none';
	if (delay < 200) return 'good';
	if (delay < 500) return 'fair';
	return 'poor';
}
