// Checks the per-version schemas for the config linter.
//
// sing-box only got an official JSON schema in 1.14, while the app supports
// 1.10.7–1.13.x. So for 1.11/1.12/1.13 we use third-party BlackDuty schemas
// (src/lib/schemas/), and for 1.14+ the official one. Here we check:
//   1. the version → schema mapping (lintSchemaForVersion) picks correctly;
//   2. a version's own schema accepts a valid config with no false errors;
//   3. the same schema catches a real typo (enum/type);
//   4. for 1.10/unknown versions there is no schema — the linter stays silent.
//
// EditorState works without a DOM, so schemaDiagnostics is checked directly.
//
// Run: task verify:editor (part of the full check).

import { EditorState } from '@codemirror/state';
import { json5 } from 'codemirror-json5';
import { lintSchemaForVersion } from '../src/lib/singbox-schemas.ts';
import { schemaDiagnostics } from '../src/lib/schema-lint.ts';

let failed = 0;
const ok = (cond, label, detail = '') => {
	console.log(`${cond ? '  ✓' : '  ✗'} ${label}${detail ? ` — ${detail}` : ''}`);
	if (!cond) failed++;
};

const stateFor = (doc) => EditorState.create({ doc, extensions: [json5()] });

// A working 1.13-style config (endpoints instead of a wireguard outbound, new DNS format).
const valid13 = `{
  "log": { "level": "info" },
  "dns": { "servers": [{ "tag": "local", "type": "tls", "server": "1.1.1.1", "detour": "direct" }], "final": "local" },
  "inbounds": [{ "type": "tun", "tag": "tun-in", "address": ["172.19.0.1/30"] }],
  "outbounds": [
    { "type": "direct", "tag": "direct" },
    { "type": "selector", "tag": "proxy", "outbounds": ["direct"] }
  ],
  "route": {
    "rules": [{ "rule_set": ["geosite-ru"], "outbound": "direct" }],
    "rule_set": [{ "type": "remote", "tag": "geosite-ru", "format": "binary", "url": "https://example.com/x.srs" }],
    "final": "proxy",
    "auto_detect_interface": true
  },
  "experimental": { "clash_api": { "external_controller": "127.0.0.1:9090" } }
}`;

const broken = `{
  "log": { "level": "nonsense" },
  "inbounds": [{ "type": "mixed", "tag": "in", "listen_port": "not-a-number" }],
  "outbounds": [{ "type": "direct", "tag": "direct" }]
}`;

// ── 1. Version → schema mapping ──────────────────────────────────────────────
console.log('\nVersion → schema mapping:');
ok(lintSchemaForVersion('1.13.7') === lintSchemaForVersion('1.13.0'), '1.13.x → one 1.13 schema');
ok(lintSchemaForVersion('1.11.0') !== lintSchemaForVersion('1.12.0'), '1.11 and 1.12 → different schemas');
ok(lintSchemaForVersion('1.14.0') === lintSchemaForVersion('1.15.2'), '1.14+ → official (one for all newer)');
ok(lintSchemaForVersion('1.10.7') === null, '1.10 — no schema (null)');
ok(lintSchemaForVersion(null) === null && lintSchemaForVersion('not-a-version') === null, 'none/unknown — null');

// ── 2. A version's own schema accepts a valid config ──────────────────────────
console.log('\nValid 1.13 config against the 1.13 schema:');
const s13 = lintSchemaForVersion('1.13.0');
const validDiags = schemaDiagnostics(stateFor(valid13), s13);
ok(validDiags.length === 0, 'no false errors', validDiags.map((d) => d.message.slice(0, 60)).join(' | '));

// ── 3. Catch a real typo ───────────────────────────────────────────────────────
console.log('\nBroken config against the 1.13 schema:');
const brokenDiags = schemaDiagnostics(stateFor(broken), s13);
const messages = brokenDiags.map((d) => d.message).join('\n');
ok(brokenDiags.length >= 1, 'errors found', `${brokenDiags.length}`);
ok(/nonsense/.test(messages), 'invalid log.level caught');
ok(/listen_port|integer/i.test(messages), 'invalid listen_port type caught');

// ── 4. No schema — the linter stays silent ────────────────────────────────────
console.log('\nNo schema (1.10/unknown):');
ok(schemaDiagnostics(stateFor(broken), lintSchemaForVersion('1.10.7')).length === 0, '1.10 — silence');
ok(schemaDiagnostics(stateFor(broken), lintSchemaForVersion(null)).length === 0, 'unknown — silence');

console.log(failed === 0 ? '\n✓ per-version schemas are fine\n' : `\n✗ checks failed: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);