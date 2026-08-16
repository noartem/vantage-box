// Как показать соединение. Чистые функции без состояния: одну и ту же строку
// рисуют и полная таблица на вкладке, и мини-панель дашборда.

import type { Connection } from './types';

/** Какой outbound несёт соединение — последний элемент цепочки. */
export function outbound(c: Connection): string {
	return c.chains.length > 0 ? c.chains[c.chains.length - 1] : '—';
}

/** Цель соединения: хост, если есть, иначе ip:port. */
export function destination(c: Connection): string {
	const m = c.metadata;
	if (m.host) return m.host;
	if (m.destinationIP) return `${m.destinationIP}:${m.destinationPort}`;
	return '—';
}

export function source(c: Connection): string {
	return `${c.metadata.sourceIP}:${c.metadata.sourcePort}`;
}

/** Полный путь в колонку не влезает, а имя файла отвечает на вопрос
 *  «какое приложение это открыло» целиком. */
export function processName(c: Connection): string {
	const path = c.metadata.processPath;
	if (!path) return '—';
	return path.split(/[\\/]/).pop() || path;
}

export function rule(c: Connection): string {
	return c.rulePayload ? `${c.rule}(${c.rulePayload})` : c.rule;
}

/** Возраст соединения на момент `now`: снимок /connections не меняет `start`,
 *  поэтому время тикает у вызывающего. */
export function age(c: Connection, now: number): number {
	const started = Date.parse(c.start);
	return Number.isNaN(started) ? 0 : now - started;
}
