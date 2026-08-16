// Разовые сообщения об ошибках действий. Постоянные проблемы (сломанный
// settings.json, недоступный API, несовместимая версия) выводятся не отсюда:
// они выводятся из состояния `app` прямо в AlertStrip и исчезают сами, когда
// причина уходит. Здесь живёт только то, у чего нет своего состояния —
// например, «не удалось остановить sing-box».

export type AlertSeverity = 'error' | 'warn' | 'ok';

export type TransientAlert = {
	id: number;
	severity: AlertSeverity;
	text: string;
};

class Transient {
	items = $state<TransientAlert[]>([]);
	private nextId = 1;

	push(severity: AlertSeverity, text: string): number {
		const id = this.nextId++;
		// Один и тот же текст подряд не копим: три одинаковых строки в счётчике
		// «1/3» — это шум, а не информация.
		this.items = [...this.items.filter((a) => a.text !== text), { id, severity, text }];
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

export function pushAlert(severity: AlertSeverity, text: string): number {
	return transientAlerts.push(severity, text);
}

export function dismissAlert(id: number) {
	transientAlerts.dismiss(id);
}
