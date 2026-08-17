import type { CompletionContext, CompletionResult } from '@codemirror/autocomplete';
import { JSONCompletion } from 'codemirror-json-schema';
import { json5Completion } from 'codemirror-json-schema/json5';

// Редактор работает в JSON5-режиме — только так CodeMirror понимает комментарии и
// висячие запятые (то есть JSONC). Но файл на диске — это JSONC/JSON, и одинарных
// кавычек бэкенд не примет. В режиме JSON5 codemirror-json-schema:
//   • getInsertTextForString    — оборачивает строки в одинарные кавычки `'…'`;
//   • getInsertTextForPropertyName — оставляет ключ без кавычек, если ничего не
//     набрано, и тут же наш jsoncLinter помечает его как ошибку.
// Перекрываем оба метода на прототипе, чтобы автокомплит всегда выдавал двойные
// кавычки — единый стандарт для всего конфига.
//
// Методы приватные в .d.ts, поэтому лезем через unknown — в собранном JS это обычные
// методы на прототипе, и экземпляр из json5Completion() их подхватывает.
const proto = JSONCompletion.prototype as unknown as {
	getInsertTextForPropertyName: (key: string) => string;
	getInsertTextForString: (value: string, prf?: string) => string;
};
proto.getInsertTextForPropertyName = (key) => `"${key}"`;
proto.getInsertTextForString = (value, prf = '#') => `"${prf}{${value}}"`;

/** Источник автокомплита от схемы sing-box, всегда с двойными кавычками. */
export function jsoncCompletion(): (ctx: CompletionContext) => CompletionResult | never[] {
	return json5Completion();
}