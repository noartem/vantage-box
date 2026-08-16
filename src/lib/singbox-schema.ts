import type { JSONSchema7 } from 'json-schema';
import generated from './singbox-schema.generated.json';

/**
 * Схема конфига sing-box для редактора.
 *
 * Файл `singbox-schema.generated.json` собирается скриптом `scripts/gen-singbox-schema.mjs`
 * (`task schema:update`) из двух источников: официальной схемы sing-box и её же
 * документации, откуда берутся подписи — в самой схеме их нет ни одной. Русские тексты
 * для часто правимых полей лежат в `scripts/singbox-schema.ru.mjs`.
 *
 * Схема отслеживает 1.14-dev, а приложение поддерживает 1.10.7–1.13.x
 * (см. `SINGBOX_MIN`/`SINGBOX_MAX_EXCLUSIVE` в `src-tauri/src/clash/client.rs`), поэтому
 * на старом синтаксисе она может ругаться там, где sing-box не против. Это осознанно:
 * ошибки схемы — подсказка в редакторе, сохранение гейтится только `sing-box check`
 * (`api.checkSingboxConfig` → `commands.rs::check_singbox_config`).
 */
// Через `unknown`: TypeScript выводит из JSON-импорта точный литеральный тип на весь
// файл, и у oneOf-вариантов взаимоисключающие поля получают `undefined`, что напрямую
// с индексной сигнатурой JSONSchema7 не сходится.
export const singboxSchema = generated as unknown as JSONSchema7;
