// Управление sing-box: одна обвязка на все места, откуда его запускают —
// вкладка «Сервис», мини-панель дашборда и статус-строка. Раньше каждая
// держала свою копию try/catch/refreshRun.

import { errorText } from './api';
import { pushAlert } from './alerts.svelte';
import { app } from './state.svelte';
import type { RestartOutcome, ServiceState } from './types';

export const SERVICE_LABELS: Record<ServiceState, string> = {
	notInstalled: 'не установлен',
	stopped: 'остановлен',
	startPending: 'запускается',
	running: 'работает',
	stopPending: 'останавливается',
	unknown: 'состояние неизвестно'
};

/** Итог перезапуска — событие, а не состояние: ему место в строке алертов,
 *  а не в баннере, который потом некому убрать. */
export function reportRestart(outcome: RestartOutcome) {
	const skipped = outcome.skipped.length > 0 ? ` Пропущено: ${outcome.skipped.join('; ')}.` : '';
	if (!outcome.apiBack) {
		pushAlert('warn', `sing-box перезапущен, но Clash API не отозвался. Проверьте логи.${skipped}`);
		return;
	}
	const restored =
		outcome.restored.length > 0
			? `Восстановлен выбор: ${outcome.restored.join(', ')}.`
			: 'Выбор selector’ов менять не пришлось.';
	pushAlert('ok', `Перезапуск завершён. ${restored}${skipped}`);
}

/** Выполняет действие над sing-box и приводит состояние в порядок.
 *  Ошибка действия — разовое событие: она уходит в строку алертов, а не растит
 *  вызывающую панель вторым баннером. */
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
