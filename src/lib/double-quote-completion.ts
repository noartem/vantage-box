import type { CompletionContext, CompletionResult } from '@codemirror/autocomplete';
import { JSONCompletion } from 'codemirror-json-schema';
import { json5Completion } from 'codemirror-json-schema/json5';

// The editor runs in JSON5 mode — that is the only way CodeMirror understands
// comments and trailing commas (i.e. JSONC). But the file on disk is JSONC/JSON,
// and the backend will not accept single quotes. In JSON5 mode codemirror-json-schema:
//   • getInsertTextForString    — wraps strings in single quotes `'…'`;
//   • getInsertTextForPropertyName — leaves the key unquoted if nothing has been
//     typed, and our jsoncLinter immediately flags that as an error.
// We override both methods on the prototype so autocomplete always produces double
// quotes — a single standard for the whole config.
//
// The methods are private in the .d.ts, so we reach in via unknown — in the built JS
// these are ordinary methods on the prototype, and the instance from json5Completion()
// picks them up.
const proto = JSONCompletion.prototype as unknown as {
	getInsertTextForPropertyName: (key: string) => string;
	getInsertTextForString: (value: string, prf?: string) => string;
};
proto.getInsertTextForPropertyName = (key) => `"${key}"`;
proto.getInsertTextForString = (value, prf = '#') => `"${prf}{${value}}"`;

/** Autocomplete source backed by the sing-box schema, always with double quotes. */
export function jsoncCompletion(): (ctx: CompletionContext) => CompletionResult | never[] {
	return json5Completion();
}