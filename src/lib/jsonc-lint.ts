import { syntaxTree } from '@codemirror/language';
import { linter, type Diagnostic } from '@codemirror/lint';
import type { EditorState } from '@codemirror/state';

/**
 * Ловит конструкции JSON5, которые не переживёт бэкенд.
 *
 * Редактор работает в режиме JSON5 — только так CodeMirror понимает комментарии и
 * висячие запятые, то есть JSONC. Но JSON5 разрешает заметно больше: одинарные кавычки,
 * ключи без кавычек, `Infinity`, hex-числа. Разбор на стороне Rust — это
 * `strip_jsonc()` (снимает только комментарии и висячие запятые) плюс `serde_json`,
 * и всё перечисленное он отвергнет.
 *
 * Без этой проверки редактор был бы добрее бэкенда: подсветка чистая, а сохранение
 * падает с «некорректный JSON». Поэтому помечаем такое сразу.
 */

const HEX_OR_SIGNED = /^[+]|^0[xX]|^\.|\.$/;
const NOT_JSON = new Set(['Infinity', '-Infinity', '+Infinity', 'NaN', '-NaN', '+NaN']);

/**
 * Отдельно от `linter()`, потому что так проверяется без DOM — хватает EditorState
 * (см. scripts/verify-jsonc-lint.mjs).
 */
export function jsoncDiagnostics(state: EditorState): Diagnostic[] {
	const diagnostics: Diagnostic[] = [];
	const text = (from: number, to: number) => state.doc.sliceString(from, to);

	syntaxTree(state).iterate({
		enter: (node) => {
			switch (node.name) {
				case 'PropertyName': {
					const raw = text(node.from, node.to);
					if (raw.startsWith("'")) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: 'Ключ в одинарных кавычках — это JSON5. sing-box и serde ждут двойные кавычки.'
						});
					} else if (!raw.startsWith('"')) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: 'Ключ без кавычек — это JSON5. Оберните имя в двойные кавычки.'
						});
					}
					break;
				}
				case 'String': {
					if (text(node.from, node.to).startsWith("'")) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: 'Строка в одинарных кавычках — это JSON5. Нужны двойные кавычки.'
						});
					}
					break;
				}
				case 'Number': {
					const raw = text(node.from, node.to);
					if (NOT_JSON.has(raw)) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: `\`${raw}\` в JSON не существует — уберите или замените числом.`
						});
					} else if (HEX_OR_SIGNED.test(raw)) {
						diagnostics.push({
							from: node.from,
							to: node.to,
							severity: 'error',
							message: `Запись \`${raw}\` — это JSON5. Нужно обычное десятичное число, например 1.5 вместо .5.`
						});
					}
					break;
				}
			}
		}
	});

	return diagnostics;
}

export const jsoncLinter = linter((view) => jsoncDiagnostics(view.state));
