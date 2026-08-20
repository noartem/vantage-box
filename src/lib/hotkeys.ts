//! Keyboard-event → accelerator-string helpers, shared between the hotkey
//! recorder in Settings and the in-app shortcut matcher in the layout.
//!
//! The string form ("Ctrl+Alt+P") matches `tauri-plugin-global-shortcut`, so a
//! recorded combination can be stored verbatim in settings.json and later
//! compared against a runtime `KeyboardEvent`.

/** Canonical modifier order — matches the plugin and the recorder. */
export const MOD_ORDER = ['Ctrl', 'Alt', 'Shift', 'Super'] as const;

/** Keys whose accelerator name differs from `KeyboardEvent.code`. */
const KEY_NAMES: Record<string, string> = {
	Escape: 'Esc',
	Backquote: '`',
	Minus: '-',
	Equal: '=',
	BracketLeft: '[',
	BracketRight: ']',
	Backslash: '\\',
	Semicolon: ';',
	Quote: "'",
	Comma: ',',
	Period: '.',
	Slash: '/'
};

/** The modifiers held during an event, in canonical order. */
export function modsFromEvent(event: KeyboardEvent): string[] {
	const mods: string[] = [];
	if (event.ctrlKey) mods.push('Ctrl');
	if (event.altKey) mods.push('Alt');
	if (event.shiftKey) mods.push('Shift');
	if (event.metaKey) mods.push('Super');
	return mods;
}

/** Whether two modifier lists describe the same set (order-independent). */
export function sameMods(a: string[], b: string[]): boolean {
	if (a.length !== b.length) return false;
	return a.every((m) => b.includes(m));
}

/** The main key of the combination. `null` — only a modifier was pressed. */
export function mainKey(code: string): string | null {
	if (/^(Control|Alt|Shift|Meta|OS)/.test(code)) return null;
	const letter = /^Key([A-Z])$/.exec(code);
	if (letter) return letter[1];
	const digit = /^Digit(\d)$/.exec(code);
	if (digit) return digit[1];
	const numpad = /^Numpad(\d)$/.exec(code);
	if (numpad) return `Numpad${numpad[1]}`;
	return KEY_NAMES[code] ?? code;
}

/** Builds the accelerator string (e.g. "Ctrl+Alt+P") from an event.
 *  Returns `null` when only modifiers are held or no modifier is held — a bare
 *  key would steal input from the whole app, so it is never treated as a
 *  shortcut. */
export function acceleratorFromEvent(event: KeyboardEvent): string | null {
	const key = mainKey(event.code);
	if (key === null) return null;
	const mods = modsFromEvent(event);
	if (mods.length === 0) return null;
	return [...mods, key].join('+');
}