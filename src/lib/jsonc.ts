/**
 * A minimal JSONC → JSON preprocessor, mirroring `src-tauri/src/jsonc.rs`
 * byte-for-byte in behavior.
 *
 * Both functions are length-preserving: comments and trailing commas become
 * spaces, so character offsets in the original text stay valid in the output —
 * diagnostics computed on the stripped text can be shown at the same spot.
 */

/** Comments and trailing commas → spaces, same length (offsets preserved 1:1). */
export function stripJsonc(input: string): string {
	let stripped = stripComments(input);
	stripped = stripTrailingCommas(stripped);
	return stripped;
}

/** Offsets of commas that are directly (whitespace/comments between) before `}` or `]`. */
export function findTrailingCommas(input: string): number[] {
	const commas: number[] = [];
	let i = 0;
	while (i < input.length) {
		if (input[i] === '"') {
			i += 1;
			while (i < input.length) {
				const ch = input[i];
				i += 1;
				if (ch === '\\') {
					i += 1;
				} else if (ch === '"') {
					break;
				}
			}
		} else if (input[i] === ',') {
			let j = i + 1;
			while (j < input.length && isJsoncWhitespace(input[j])) j += 1;
			if (j < input.length && (input[j] === '}' || input[j] === ']')) {
				commas.push(i);
			}
			i += 1;
		} else {
			i += 1;
		}
	}
	return commas;
}

function stripComments(input: string): string {
	let out = '';
	let i = 0;

	while (i < input.length) {
		const ch = input[i];
		if (ch === '"') {
			// Copy a string literal as-is, respecting escaping.
			out += ch;
			i += 1;
			while (i < input.length) {
				const c = input[i];
				out += c;
				i += 1;
				if (c === '\\') {
					if (i < input.length) {
						out += input[i];
						i += 1;
					}
				} else if (c === '"') {
					break;
				}
			}
		} else if (ch === '/' && input[i + 1] === '/') {
			while (i < input.length && input[i] !== '\n') {
				out += ' ';
				i += 1;
			}
		} else if (ch === '/' && input[i + 1] === '*') {
			out += '  ';
			i += 2;
			while (i < input.length) {
				if (input[i] === '*' && input[i + 1] === '/') {
					out += '  ';
					i += 2;
					break;
				}
				// Keep newlines, turn everything else into a space.
				out += input[i] === '\n' ? '\n' : ' ';
				i += 1;
			}
		} else {
			out += ch;
			i += 1;
		}
	}

	return out;
}

function stripTrailingCommas(input: string): string {
	const chars = input.split('');
	let i = 0;

	while (i < input.length) {
		if (input[i] === '"') {
			i += 1;
			while (i < input.length) {
				const ch = input[i];
				i += 1;
				if (ch === '\\') {
					i += 1;
				} else if (ch === '"') {
					break;
				}
			}
		} else if (input[i] === ',') {
			let j = i + 1;
			while (j < input.length && isJsoncWhitespace(input[j])) j += 1;
			if (j < input.length && (input[j] === '}' || input[j] === ']')) {
				chars[i] = ' ';
			}
			i += 1;
		} else {
			i += 1;
		}
	}

	return chars.join('');
}

const isJsoncWhitespace = (ch: string) => ' \t\n\u000b\u000c\r'.includes(ch);

