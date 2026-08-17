// Проверка per-version схем для линтера конфига.
//
// Официальная JSON-схема у sing-box появилась только в 1.14, а приложение поддерживает
// 1.10.7–1.13.x. Поэтому для 1.11/1.12/1.13 используем сторонние схемы BlackDuty
// (src/lib/schemas/), а для 1.14+ — официальную. Здесь проверяем:
//   1. mapping версия → схема (lintSchemaForVersion) выбирает правильно;
//   2. схема своей версии принимает валидный конфиг без ложных ошибок;
//   3. та же схема ловит настоящую опечатку (enum/type);
//   4. для 1.10/неизвестной версии схемы нет — линтер молчит.
//
// EditorState работает без DOM, поэтому schemaDiagnostics проверяется напрямую.
//
// Запуск: task verify:editor (входит в общую проверку).

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

// Рабочий конфиг в стиле 1.13 (endpoints вместо wireguard-outbound, новый DNS-формат).
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
  "log": { "level": "ерунда" },
  "inbounds": [{ "type": "mixed", "tag": "in", "listen_port": "не-число" }],
  "outbounds": [{ "type": "direct", "tag": "direct" }]
}`;

// ── 1. Маппинг версия → схема ────────────────────────────────────────────────
console.log('\nМаппинг версия → схема:');
ok(lintSchemaForVersion('1.13.7') === lintSchemaForVersion('1.13.0'), '1.13.x → одна схема 1.13');
ok(lintSchemaForVersion('1.11.0') !== lintSchemaForVersion('1.12.0'), '1.11 и 1.12 → разные схемы');
ok(lintSchemaForVersion('1.14.0') === lintSchemaForVersion('1.15.2'), '1.14+ → официальная (одна на всех новыше)');
ok(lintSchemaForVersion('1.10.7') === null, '1.10 — схемы нет (null)');
ok(lintSchemaForVersion(null) === null && lintSchemaForVersion('не-версия') === null, 'нет/unknown — null');

// ── 2. Схема своей версии принимает валидный конфиг ──────────────────────────
console.log('\nВалидный 1.13-конфиг против схемы 1.13:');
const s13 = lintSchemaForVersion('1.13.0');
const validDiags = schemaDiagnostics(stateFor(valid13), s13);
ok(validDiags.length === 0, 'без ложных ошибок', validDiags.map((d) => d.message.slice(0, 60)).join(' | '));

// ── 3. Ловим настоящую опечатку ───────────────────────────────────────────────
console.log('\nБитый конфиг против схемы 1.13:');
const brokenDiags = schemaDiagnostics(stateFor(broken), s13);
const messages = brokenDiags.map((d) => d.message).join('\n');
ok(brokenDiags.length >= 1, 'ошибки найдены', `${brokenDiags.length}`);
ok(/ерунда/.test(messages), 'неверный log.level пойман');
ok(/listen_port|Не соответствует|integer/i.test(messages), 'неверный тип listen_port пойман');

// ── 4. Нет схемы — линтер молчит ─────────────────────────────────────────────
console.log('\nБез схемы (1.10/unknown):');
ok(schemaDiagnostics(stateFor(broken), lintSchemaForVersion('1.10.7')).length === 0, '1.10 — тишина');
ok(schemaDiagnostics(stateFor(broken), lintSchemaForVersion(null)).length === 0, 'unknown — тишина');

console.log(failed === 0 ? '\n✓ per-version схемы в порядке\n' : `\n✗ провалено проверок: ${failed}\n`);
process.exit(failed === 0 ? 0 : 1);