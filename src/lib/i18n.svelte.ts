// i18n bootstrap: language list, system auto-detection and preference persistence.
//
// Detection mirrors the twentymate app (a .NET/Avalonia project whose logic we
// ported here): build a candidate list from the OS/browser preferred UI languages,
// match each against the shipped locales — exact BCP-47 tag, then neutral
// two-letter, then regional prefix (so a bare `pt` preference resolves to the
// shipped `pt-BR`) — and fall back to English.
//
// We keep the "system" preference ourselves in localStorage (`vb.lang`) and drive
// Paraglide's runtime via setLocale(code, { reload: false }). Paraglide's strategy
// is globalVariable + baseLocale, so setLocale switches the in-memory locale that
// the generated m.x() message functions read, without touching any URL or
// fighting the "system follows OS" semantics. Switching the language reloads the
// document (m.x() is not reactive), which is fine for a desktop app — the Rust
// backend and its state survive.

import { setLocale, getLocale, locales, baseLocale, type Locale } from '$lib/paraglide/runtime.js';

/** Shipped locales, in selector order. English is the fallback. */
export const LOCALES = locales;
export const BASE_LOCALE: Locale = baseLocale;

const LANG_STORAGE_KEY = 'vb.lang';

/** A storable preference: "system" (auto-detect) or a concrete locale. */
export type LanguagePreference = 'system' | Locale;

/**
 * Selector options. Locale names are shown in their own language (like twentymate):
 * a label cannot be translated before the locale it names is chosen, and native
 * names stay recognizable regardless of the active UI language.
 */
export const LANGUAGE_OPTIONS: ReadonlyArray<{ value: LanguagePreference; label: string }> = [
	{ value: 'system', label: 'System' },
	{ value: 'en', label: 'English' },
	{ value: 'ru', label: 'Русский' },
	{ value: 'es', label: 'Español' },
	{ value: 'de', label: 'Deutsch' },
	{ value: 'fr', label: 'Français' },
	{ value: 'pt-BR', label: 'Português (Brasil)' }
];

function isLocaleValue(value: unknown): value is Locale {
	return ((LOCALES as readonly string[]).includes(value as string));
}

/** The stored preference, defaulting to "system". */
export function getLanguagePreference(): LanguagePreference {
	if (typeof localStorage === 'undefined') return 'system';
	const saved = localStorage.getItem(LANG_STORAGE_KEY);
	if (saved === 'system' || isLocaleValue(saved)) return saved as LanguagePreference;
	return 'system';
}

/** Match a single BCP-47 tag against the shipped locales:
 *  exact tag, then neutral two-letter, then regional prefix (pt -> pt-BR). */
function matchShipped(tag: string): Locale | null {
	if (!tag) return null;
	const lower = tag.toLowerCase();
	for (const l of LOCALES) if (l.toLowerCase() === lower) return l;
	const neutral = lower.split('-')[0];
	for (const l of LOCALES) {
		const ll = l.toLowerCase();
		if (ll === neutral || ll.startsWith(neutral + '-')) return l;
	}
	return null;
}

/** Resolve "system" to a concrete locale via the browser/OS preferred UI languages. */
export function detectSystemLocale(): Locale {
	if (typeof navigator === 'undefined') return BASE_LOCALE;
	const candidates =
		navigator.languages && navigator.languages.length > 0
			? [...navigator.languages]
			: typeof navigator.language === 'string'
				? [navigator.language]
				: [];
	for (const candidate of candidates) {
		const match = matchShipped(candidate);
		if (match) return match;
	}
	return BASE_LOCALE;
}

/** Resolve a preference (system or concrete) to a concrete locale. */
export function resolveLocale(pref: LanguagePreference): Locale {
	return pref === 'system' ? detectSystemLocale() : pref;
}

/** The locale currently in effect (concrete, never "system"). */
export function currentLocale(): Locale {
	return getLocale();
}

/** Apply the stored preference to Paraglide's runtime. Call once at app init,
 *  before the first render, so there is no flash of the base locale. Browser-only
 *  (relies on localStorage / navigator). */
export function applyLocale(): void {
	if (typeof localStorage === 'undefined') return;
	const resolved = resolveLocale(getLanguagePreference());
	if (getLocale() !== resolved) setLocale(resolved, { reload: false });
	if (typeof document !== 'undefined') document.documentElement.lang = resolved;
}

/** Persist a language choice and reload so the whole UI re-renders in the new
 *  language. The reload is skipped (and only the runtime is updated) when the
 *  resolved locale would not change. */
export function setLanguagePreference(pref: LanguagePreference): void {
	if (typeof localStorage !== 'undefined') localStorage.setItem(LANG_STORAGE_KEY, pref);
	const resolved = resolveLocale(pref);
	if (typeof location !== 'undefined' && resolved !== currentLocale()) {
		location.reload();
	} else {
		setLocale(resolved, { reload: false });
		if (typeof document !== 'undefined') document.documentElement.lang = resolved;
	}
}