/// <reference lib="webworker" />
// Воркер, выносящий валидацию конфига по схеме за пределы главного потока.
//
// Схемный линтер — самый тяжёлый из трёх: он разбирает весь документ и гоняет его
// через Draft07.validate против схемы в сотни КБ. На главном потоке это давало
// фризы при наборе. Здесь же разбор (parseJSON5DocumentState через json5.parse +
// Lezer-дерево) и валидация выполняются в воркере — главный поток свободен.
//
// Диагностика считается той же чистой функцией schemaDiagnostics, что и на главном
// потоке (src/lib/schema-lint.ts), поэтому смещения from/to совпадают с тем, что
// было раньше. Схему присылает главный поток при смене версии sing-box
// (setSchema), а lint-запросы шлёт асинхронный линтер в CodeEditor.

import { EditorState } from '@codemirror/state';
import type { Diagnostic } from '@codemirror/lint';
import { json5 } from 'codemirror-json5';
import type { JSONSchema7 } from 'json-schema';
import { schemaDiagnostics } from './schema-lint';

let schema: JSONSchema7 | null = null;

type InMessage =
	| { type: 'setSchema'; schema: JSONSchema7 | null }
	| { type: 'lint'; id: number; text: string };

type OutMessage = { type: 'lintResult'; id: number; diags: Diagnostic[] };

self.onmessage = (event: MessageEvent<InMessage>) => {
	const msg = event.data;
	if (msg.type === 'setSchema') {
		schema = msg.schema;
		return;
	}
	if (msg.type === 'lint') {
		const { id, text } = msg;
		const diags =
			schema && text.length
				? schemaDiagnostics(EditorState.create({ doc: text, extensions: [json5()] }), schema)
				: [];
		const out: OutMessage = { type: 'lintResult', id, diags };
		(self as unknown as Worker).postMessage(out);
	}
};