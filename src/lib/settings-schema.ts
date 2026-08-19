import type { JSONSchema7 } from 'json-schema';
// The canonical settings schema lives with the backend (src-tauri/schemas), which
// also writes it to disk next to settings.json (include_str! in write_schema).
// Imported here directly so the in-app editor lints and offers hover tooltips
// against the very schema the backend enforces — one source of truth, no copy to drift.
import raw from '../../src-tauri/schemas/settings.schema.json';

export const settingsSchema = raw as unknown as JSONSchema7;