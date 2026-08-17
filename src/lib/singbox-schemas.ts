import type { JSONSchema7 } from 'json-schema';
// Официальная схема sing-box 1.14-dev — единственная, что существует официально
// (команда `sing-box schema` и схема на сайте появились только в 1.14.0). У неё есть
// русские подписи (singbox-schema.ru.mjs) и union-трансформ для автокомплита, поэтому
// её же используем для автокомплита/hover во вкладке конфига при любой версии —
// подсказки по полям в основном версионно-нейтральны.
import generated from './singbox-schema.generated.json';
// Сторонние per-version схемы для 1.11–1.13 (официальных для этих версий нет).
// Поддерживаются сообществом в репо BlackDuty/sing-box-schema, собираются из Zod.
import s111 from './schemas/singbox-1.11.1.json';
import s112 from './schemas/singbox-1.12.22.json';
import s113 from './schemas/singbox-1.13.13.json';

const asSchema = (s: unknown) => s as unknown as JSONSchema7;

/** Схема для автокомплита/hover — всегда официальная 1.14 (с русскими подписями). */
export const autocompleteSchema = asSchema(generated);

// Минор → схема для линтера. Ключ `major*100+minor`, чтобы 1.11 ≠ 1.1 и т.п.
const BY_MINOR: Record<number, JSONSchema7> = {
	111: asSchema(s111),
	112: asSchema(s112),
	113: asSchema(s113),
	114: autocompleteSchema
};

/**
 * Схема для линтера по версии запущенного sing-box.
 *
 * - 1.11/1.12/1.13 — сторонняя схема соответствующего минора (BlackDuty).
 * - 1.14+ — официальная 1.14-dev (единственная, что есть).
 * - 1.10.x и младше, либо неизвестная версия — `null`: подходящей схемы нет,
 *   линтер выключается, ложных ошибок не будет. Реальный гейт — `sing-box check`.
 *
 * Для версий новыше 1.14 берём 1.14-dev как ближайшее, что есть.
 */
export function lintSchemaForVersion(raw: string | null | undefined): JSONSchema7 | null {
	if (!raw) return null;
	const match = /(\d+)\.(\d+)/.exec(raw);
	if (!match) return null;
	const minor = Number(match[1]) * 100 + Number(match[2]);

	if (BY_MINOR[minor]) return BY_MINOR[minor];
	if (minor < 111) return null; // 1.10 и младше — схемы нет
	return autocompleteSchema; // новыше 1.14 — ближайшая известная
}