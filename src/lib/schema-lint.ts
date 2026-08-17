import { linter, type Diagnostic } from '@codemirror/lint';
import type { EditorState } from '@codemirror/state';
import { Draft07, type JsonError } from 'json-schema-library';
import { parseJSON5DocumentState } from 'codemirror-json-schema/json5';
import type { JSONSchema7 } from 'json-schema';

/**
 * Схемный линтер для редактора конфига sing-box.
 *
 * Валидирует документ против **схемы, соответствующей запущенной версии** sing-box
 * (см. `singbox-schemas.ts`): 1.11/1.12/1.13 — сторонние BlackDuty, 1.14+ — официальная.
 * Фильтровать структурный шум не нужно — схема просто подходит к конфигу этой версии.
 * Когда подходящей схемы нет (1.10.x, неизвестная версия), передаём `null` — линтер
 * молчит: реальный гейт сохранения — `sing-box check`.
 *
 * Draft07, а не Draft04: сторонние схемы собраны под draft 2020-12, и Draft04 на их
 * `oneOf`/`const` падает с `Cannot read properties of undefined`. Официальная 1.14
 * под Draft07 валидирует так же, как под Draft04 (см. verify-singbox-schema).
 *
 * Геттер вместо готовой схемы — чтобы менять схему на лету при смене версии sing-box
 * без пересоздания редактора: CodeEditor обновляет переменную и вызывает `forceLinting`.
 */

/** Ошибки, указывающие на ключ свойства, а не на его значение. */
const KEY_ERRORS = new Set([
	'NoAdditionalPropertiesError',
	'RequiredPropertyError',
	'InvalidPropertyNameError',
	'ForbiddenPropertyError',
	'UndefinedValueError'
]);

function errorPath(error: JsonError): string {
	const data = error.data;
	if (data?.pointer && data.pointer !== '#') return data.pointer.slice(1);
	if (data?.property) return `/${data.property}`;
	return '';
}

function rewrite(error: JsonError): string {
	if (error.code === 'type-error') {
		const expected = error.data?.expected;
		const exp = Array.isArray(expected) ? expected.join(' или ') : (expected ?? '');
		const got = error.data?.received ?? '';
		return `Ожидался ${exp}, получен ${got}`.replace(/\s+$/, '');
	}
	if (error.code === 'one-of-error' || error.code === 'any-of-error') {
		const pointer = error.data?.pointer ?? '';
		return `Не соответствует ни одному варианту${pointer ? ` (${pointer})` : ''}`;
	}
	return (error.message ?? '')
		.replaceAll('in `#` ', '')
		.replaceAll('at `#`', '')
		.replaceAll('/', '.')
		.replaceAll('#.', '')
		.trim();
}

// Драфт тяжёлый (схема сотни КБ), а линт бежит на каждый чих — кэшируем по схеме.
const draftCache = new WeakMap<JSONSchema7, Draft07>();
function getDraft(schema: JSONSchema7): Draft07 {
	let draft = draftCache.get(schema);
	if (!draft) {
		draft = new Draft07(schema as never);
		draftCache.set(schema, draft);
	}
	return draft;
}

/**
 * Чистая функция диагностики по EditorState — вынесена отдельно, чтобы проверять
 * без DOM (см. scripts/verify-singbox-schemas.mjs). `schema === null` — валидации нет.
 */
export function schemaDiagnostics(state: EditorState, schema: JSONSchema7 | null | undefined): Diagnostic[] {
	if (!schema) return [];
	const text = state.doc.toString();
	if (!text.length) return [];

	const { data, pointers } = parseJSON5DocumentState(state);
	if (data == null) return [];

	let errors: JsonError[] = [];
	try {
		errors = (getDraft(schema).validate(data) as JsonError[]) ?? [];
	} catch {
		// Сторонняя схема может содержать узлы, которые json-schema-library не
		// переварит на конкретном конфиге — лучше тихо пропустить, чем уронить редактор.
		return [];
	}

	const diags: Diagnostic[] = [];
	for (const error of errors) {
		if (!error || typeof error.name !== 'string') continue;

		const path = errorPath(error);
		const message = rewrite(error);

		if (path === '' || error.name === 'MaxPropertiesError' || error.name === 'MinPropertiesError') {
			diags.push({ from: 0, to: 0, message, severity: 'error', source: 'sing-box' });
			continue;
		}

		const pointer = pointers.get(path) as
			| { keyFrom?: number; keyTo?: number; valueFrom?: number; valueTo?: number }
			| undefined;
		if (pointer) {
			const isKey = KEY_ERRORS.has(error.name);
			// Значение-ошибка подчёркивает valueFrom/valueTo, но для ошибок на целом
			// узле-объекте/массиве (AnyOfError на inbounds[0]) их нет — есть только
			// keyFrom/keyTo, охватывающие весь узел. Падаем на них, иначе диагностика
			// молча теряется. ?? , а не || : valueFrom может быть 0.
			const from = isKey ? pointer.keyFrom : pointer.valueFrom ?? pointer.keyFrom;
			const to = isKey ? pointer.keyTo : pointer.valueTo ?? pointer.keyTo;
			if (from !== undefined && to !== undefined) {
				diags.push({ from, to, message, severity: 'error', source: 'sing-box' });
			}
		} else {
			diags.push({ from: 0, to: 0, message, severity: 'error', source: 'sing-box' });
		}
	}
	return diags;
}

export function schemaLinter(getSchema: () => JSONSchema7 | null | undefined) {
	return linter((view) => schemaDiagnostics(view.state, getSchema()));
}