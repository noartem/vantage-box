import type { JSONSchema7 } from 'json-schema';
// The official sing-box 1.14-dev schema — the only one that exists officially
// (the `sing-box schema` command and the on-site schema appeared only in 1.14.0). It has
// Manual descriptions (singbox-schema-overlay.mjs) and a union transform for autocomplete,
// so we also use it for autocomplete/hover in the config tab regardless of version —
// the field descriptions are mostly version-neutral.
import generated from './singbox-schema.generated.json';
// Third-party per-version schemas for 1.11–1.13 (no official ones exist for these).
// Maintained by the community in the BlackDuty/sing-box-schema repo, built from Zod.
import s111 from './schemas/singbox-1.11.1.json';
import s112 from './schemas/singbox-1.12.22.json';
import s113 from './schemas/singbox-1.13.13.json';

const asSchema = (s: unknown) => s as unknown as JSONSchema7;

/** Schema for autocomplete/hover — always the official 1.14 (with manual descriptions). */
export const autocompleteSchema = asSchema(generated);

// Minor → schema for the linter. Key is `major*100+minor`, so 1.11 ≠ 1.1 etc.
const BY_MINOR: Record<number, JSONSchema7> = {
	111: asSchema(s111),
	112: asSchema(s112),
	113: asSchema(s113),
	114: autocompleteSchema
};

/**
 * Schema for the linter based on the running sing-box version.
 *
 * - 1.11/1.12/1.13 — third-party schema for the corresponding minor (BlackDuty).
 * - 1.14+ — official 1.14-dev (the only one that exists).
 * - 1.10.x and below, or an unknown version — `null`: no matching schema, the
 *   linter is disabled, no false errors. The real gate is `sing-box check`.
 *
 * For versions newer than 1.14 we use 1.14-dev as the closest available.
 */
export function lintSchemaForVersion(raw: string | null | undefined): JSONSchema7 | null {
	if (!raw) return null;
	const match = /(\d+)\.(\d+)/.exec(raw);
	if (!match) return null;
	const minor = Number(match[1]) * 100 + Number(match[2]);

	if (BY_MINOR[minor]) return BY_MINOR[minor];
	if (minor < 111) return null; // 1.10 and below — no schema
	return autocompleteSchema; // newer than 1.14 — closest known
}