const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** Человекочитаемый объём: 0 B, 812 B, 1.4 MB. */
export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < UNITS.length - 1) {
		value /= 1024;
		unit += 1;
	}
	// Байты целые, всё остальное — с одним знаком: столбик цифр не «прыгает».
	return `${unit === 0 ? Math.round(value) : value.toFixed(1)} ${UNITS[unit]}`;
}

/** Clash отдаёт трафик как «за последнюю секунду», поэтому это сразу скорость. */
export function formatSpeed(bytesPerSecond: number): string {
	return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatTime(millis: number): string {
	const d = new Date(millis);
	const pad = (n: number, width = 2) => String(n).padStart(width, '0');
	return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

/** Возраст соединения. Колонка узкая, поэтому единица всегда одна. */
export function formatDuration(millis: number): string {
	if (!Number.isFinite(millis) || millis < 0) return '—';
	const seconds = Math.floor(millis / 1000);
	if (seconds < 60) return `${seconds}с`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}м`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}ч`;
	return `${Math.floor(hours / 24)}д`;
}

/** Задержка узла: null — не измеряли, 0 и меньше — узел не ответил. */
export function formatDelay(delay: number | null): string {
	if (delay === null || delay <= 0) return '—';
	return `${delay} ms`;
}

/** Порог для цветовой маркировки задержки. */
export function delayTone(delay: number | null): 'none' | 'good' | 'fair' | 'poor' {
	if (delay === null || delay <= 0) return 'none';
	if (delay < 200) return 'good';
	if (delay < 500) return 'fair';
	return 'poor';
}
