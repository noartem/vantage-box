/// <reference lib="webworker" />
// Worker that moves schema-based config validation off the main thread.
//
// The schema linter is the heaviest of the three: it parses the whole document and
// runs it through Draft07.validate against a schema of hundreds of KB. On the main
// thread this caused typing freezes. Here the parsing (parseJSON5DocumentState via
// json5.parse + Lezer tree) and validation run in the worker — the main thread is free.
//
// Diagnostics are computed by the same pure schemaDiagnostics function used on the
// main thread (src/lib/schema-lint.ts), so the from/to offsets match what they were
// before. The schema is sent by the main thread when the sing-box version changes
// (setSchema), and lint requests are issued by the async linter in CodeEditor.

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