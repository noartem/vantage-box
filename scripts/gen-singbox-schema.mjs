// Генератор схемы конфига sing-box для редактора.
//
// Зачем вообще генератор:
//   1. У sing-box есть официальная схема (с 1.14.0-beta.2), но в ней НОЛЬ описаний —
//      4805 узлов, ни одного `description`. Автокомплит она даёт, подсказки — нет.
//      Тексты приходится собирать из документации SagerNet (markdown, `#### поле`).
//   2. Схема описывает inbounds/outbounds/route.rules как `oneOf` вариантов с
//      дискриминатором `type`. json-schema-library (на ней работает codemirror-json-schema)
//      на таком узле возвращает голый `oneOf` без `properties` — то есть ни автокомплита,
//      ни подсказок ровно там, где они нужнее всего. Лечится union-трансформом ниже.
//
// Запуск: task schema:update  (или npm run schema:update)
// Результат: src/lib/singbox-schema.generated.json — коммитится в репозиторий,
// чтобы сборка не зависела от сети.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ruOverlay } from './singbox-schema.ru.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT = path.join(ROOT, 'src', 'lib', 'singbox-schema.generated.json');

const SCHEMA_URL = 'https://sing-box.sagernet.org/schema.json';
// Ветка, в которой лежит документация под ту же версию, что и опубликованная схема.
const DOCS_REF = 'dev-next-wip';
const DOCS_PREFIX = 'docs/configuration/';

// ─────────────────────────────────────────────────────────────────────────────
// Загрузка
// ─────────────────────────────────────────────────────────────────────────────

async function fetchText(url) {
	const res = await fetch(url, { headers: { 'user-agent': 'vantage-box-schema-generator' } });
	if (!res.ok) throw new Error(`${res.status} ${res.statusText} — ${url}`);
	return res.text();
}

async function fetchDocs() {
	const tree = JSON.parse(
		await fetchText(`https://api.github.com/repos/SagerNet/sing-box/git/trees/${DOCS_REF}?recursive=1`)
	);
	if (!tree.tree) throw new Error(`не удалось получить дерево репозитория: ${JSON.stringify(tree).slice(0, 200)}`);

	const files = tree.tree
		.map((f) => f.path)
		.filter((p) => p.startsWith(DOCS_PREFIX) && p.endsWith('.md') && !p.includes('.zh.'))
		.sort();

	const docs = {};
	// Небольшими пачками, чтобы не долбить raw.githubusercontent сотней параллельных запросов.
	for (let i = 0; i < files.length; i += 8) {
		const batch = files.slice(i, i + 8);
		const texts = await Promise.all(
			batch.map((p) => fetchText(`https://raw.githubusercontent.com/SagerNet/sing-box/${DOCS_REF}/${p}`))
		);
		batch.forEach((p, j) => {
			docs[p.slice(DOCS_PREFIX.length)] = texts[j];
		});
	}
	return docs;
}

// ─────────────────────────────────────────────────────────────────────────────
// Разбор документации: `#### имя_поля` → описание
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Достаёт из markdown-страницы карту «имя поля → описание».
 * Учитывает особенности разметки SagerNet:
 *   - `!!! question "Since sing-box 1.8.0"` → приписываем к описанию курсивом;
 *   - `==Required==` → это маркер, а не текст: снимаем в пометку и берём следующий абзац;
 *   - ссылки `[текст](url)` схлопываем в текст (в тултипе URL всё равно не кликнешь).
 */
export function parseDoc(md) {
	const out = {};
	const lines = md.split(/\r?\n/);
	let field = null;
	let buf = [];

	const flush = () => {
		if (field && !out[field]) {
			const desc = buildDescription(buf);
			if (desc) out[field] = desc;
		}
		field = null;
		buf = [];
	};

	for (const line of lines) {
		const heading = line.match(/^####\s+`?([A-Za-z0-9_.]+)`?\s*$/);
		if (heading) {
			flush();
			field = heading[1];
			continue;
		}
		// Любой другой заголовок закрывает текущее поле.
		if (/^#{1,6}\s/.test(line)) {
			flush();
			continue;
		}
		if (field !== null) buf.push(line);
	}
	flush();
	return out;
}

function buildDescription(buf) {
	const notes = [];
	const text = [];
	let required = false;

	for (const line of buf) {
		const admonition = line.match(/^!!!\s+\w+\s+"([^"]+)"/);
		if (admonition) {
			notes.push(admonition[1]);
			continue;
		}
		if (/^\s{4}/.test(line)) continue; // тело admonition-а
		if (/^===\s/.test(line)) continue; // переключатели вкладок
		if (/^==.+==\s*$/.test(line.trim())) {
			if (/required/i.test(line)) required = true;
			continue;
		}
		text.push(line);
	}

	// Первый непустой абзац — это и есть описание.
	const body = text
		.join('\n')
		.split(/\n\s*\n/)
		.map((p) => p.trim())
		.find((p) => p.length > 0);

	let desc = (body ?? '')
		.replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // ссылки → текст
		.replace(/\s*\n\s*/g, ' ')
		.trim();

	if (required) desc = desc ? `**Обязательное.** ${desc}` : '**Обязательное.**';
	if (!desc && notes.length === 0) return null;
	return [desc, ...notes.map((n) => `*${n}*`)].filter(Boolean).join('\n\n');
}

// ─────────────────────────────────────────────────────────────────────────────
// Раскладка описаний по схеме
// ─────────────────────────────────────────────────────────────────────────────

// Держатели tagged-union-ов. В официальной схеме общие опции слушателя и дайлера
// НЕ вынесены в отдельный $def, а продублированы внутри каждого варианта (listen_port —
// 37 объявлений, detour — 80). Поэтому страницы из shared/ раскладываем по этим узлам
// целиком: applyDescriptions пройдёт по всем вариантам сам.
const UNION_HOLDERS = ['Inbound', 'Outbound', 'Endpoint', 'Service', 'DNSServer'];

// Правила с action "route"/"resolve" принимают те же опции дайлера (bind_interface,
// tcp_fast_open, connect_timeout и прочее) — в схеме они тоже продублированы внутрь.
const ACTION_HOLDERS = ['Rule', 'NestedRule', 'RuleAction', 'DNSRule', 'NestedDNSRule', 'DNSRuleAction'];

// Какой файл документации описывает какие $defs. Для inbound/outbound таблица не нужна:
// имя файла совпадает со значением `type`, по нему и находим нужный вариант.
const TARGETS = {
	'log/index.md': ['LogOptions'],
	'ntp/index.md': ['NTPOptions'],
	'dns/index.md': ['DNS'],
	'dns/server.md': ['DNSServer'],
	'dns/rule.md': ['DNSRule', 'NestedDNSRule', 'DNSRuleAction'],
	'dns/fakeip.md': ['DNS'],
	'route/index.md': ['RouteOptions'],
	'route/rule.md': ['Rule', 'NestedRule', 'RuleAction'],
	'route/sniff.md': ['RuleAction', 'DNSRuleAction'],
	'rule-set/index.md': ['RuleSet'],
	'rule-set/headless-rule.md': ['HeadlessRule'],
	'rule-set/source-format.md': ['RuleSet'],
	'experimental/index.md': ['ExperimentalOptions'],
	'experimental/cache-file.md': ['CacheFileOptions'],
	'experimental/clash-api.md': ['ClashAPIOptions'],
	'experimental/v2ray-api.md': ['V2RayAPIOptions', 'V2RayStatsServiceOptions'],
	'shared/dial.md': ['DialerOptions', ...UNION_HOLDERS, ...ACTION_HOLDERS],
	'shared/listen.md': UNION_HOLDERS,
	'shared/tls.md': [
		'InboundTLSOptions',
		'OutboundTLSOptions',
		'InboundRealityOptions',
		'OutboundRealityOptions',
		'InboundRealityHandshakeOptions',
		'OutboundUTLSOptions',
		'InboundECHOptions',
		'OutboundECHOptions',
		'ACMEProviderDNS01Challenge'
	],
	'shared/dns01_challenge.md': ['ACMEProviderDNS01Challenge'],
	'shared/multiplex.md': ['InboundMultiplexOptions', 'OutboundMultiplexOptions'],
	'shared/v2ray-transport.md': ['V2RayTransport'],
	'shared/tcp-brutal.md': ['BrutalOptions'],
	'shared/udp-over-tcp.md': ['DialerOptions', ...UNION_HOLDERS]
};

/** Свойства варианта с раскрытием allOf/$ref — та же логика, что и в union-трансформе. */
function collectProps(defs, node, seen = new Set(), depth = 0) {
	if (!node || typeof node !== 'object' || depth > 16) return {};
	if (node.$ref) {
		const key = node.$ref.replace('#/$defs/', '');
		if (seen.has(key) || !defs[key]) return {};
		seen.add(key);
		return collectProps(defs, defs[key], seen, depth + 1);
	}
	const out = { ...(node.properties ?? {}) };
	for (const part of node.allOf ?? []) Object.assign(out, collectProps(defs, part, seen, depth + 1));
	for (const part of node.oneOf ?? []) Object.assign(out, collectProps(defs, part, new Set(seen), depth + 1));
	for (const part of node.anyOf ?? []) Object.assign(out, collectProps(defs, part, new Set(seen), depth + 1));
	return out;
}

/** Значения `type`, которые покрывает вариант tagged-union-а. */
function variantTypes(defs, variant) {
	const props = collectProps(defs, variant);
	const t = props.type;
	if (!t) return [];
	if (Array.isArray(t.enum)) return t.enum.filter(Boolean);
	if (t.const) return [t.const];
	return [];
}

/**
 * Расставляет описания внутри поддерева. По $ref не ходим — чужие $defs описываются
 * своим файлом документации, иначе tun-овские тексты расползутся по всем inbound-ам.
 */
function applyDescriptions(node, map, stats, depth = 0) {
	if (!node || typeof node !== 'object' || depth > 24) return;
	if (Array.isArray(node)) {
		for (const item of node) applyDescriptions(item, map, stats, depth + 1);
		return;
	}
	if (node.properties && typeof node.properties === 'object') {
		for (const [name, sub] of Object.entries(node.properties)) {
			if (sub && typeof sub === 'object' && !sub.description && map[name]) {
				sub.description = map[name];
				stats.applied++;
			}
		}
	}
	for (const [key, value] of Object.entries(node)) {
		if (key === '$ref' || key === 'description') continue;
		if (value && typeof value === 'object') applyDescriptions(value, map, stats, depth + 1);
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Union-трансформ: то, ради чего всё затевалось
// ─────────────────────────────────────────────────────────────────────────────

/**
 * У каждого `oneOf`-узла проставляет `properties` = объединение свойств всех вариантов.
 * Сам `oneOf` остаётся нетронутым, поэтому валидация не слабеет: `oneOf` по-прежнему
 * отсекает несуществующие комбинации, а `properties` даёт резолверу за что зацепиться
 * при автокомплите и hover-подсказках.
 */
function addUnionProps(defs, node, stats) {
	if (!node || typeof node !== 'object') return;
	if (Array.isArray(node)) {
		for (const item of node) addUnionProps(defs, item, stats);
		return;
	}
	if (Array.isArray(node.oneOf) && !node.properties) {
		const union = {};
		for (const variant of node.oneOf) Object.assign(union, collectProps(defs, variant));
		if (Object.keys(union).length) {
			node.type = node.type ?? 'object';
			node.properties = union;
			stats.unions++;
		}
	}
	for (const value of Object.values(node)) {
		if (value && typeof value === 'object') addUnionProps(defs, value, stats);
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Русский оверлей
// ─────────────────────────────────────────────────────────────────────────────

function applyOverlay(schema, stats) {
	const defs = schema.$defs;
	const missed = [];

	for (const [key, text] of Object.entries(ruOverlay)) {
		const sep = key.lastIndexOf('.');
		const scope = key.slice(0, sep);
		const field = key.slice(sep + 1);

		let targets = [];
		if (scope === '#') {
			targets = [schema];
		} else if (scope.startsWith('inbound:') || scope.startsWith('outbound:')) {
			const [kind, type] = scope.split(':');
			const holder = defs[kind === 'inbound' ? 'Inbound' : 'Outbound'];
			targets = (holder?.oneOf ?? []).filter((v) => variantTypes(defs, v).includes(type));
		} else if (defs[scope]) {
			targets = [defs[scope]];
		}

		if (!targets.length) {
			missed.push(key);
			continue;
		}

		let hit = false;
		for (const target of targets) {
			// Ищем свойство в самом узле и во всех его вариантах/частях.
			for (const holder of propertyHolders(defs, target)) {
				if (holder[field]) {
					holder[field].description = text;
					hit = true;
				}
			}
		}
		if (hit) stats.overlay++;
		else missed.push(key);
	}
	return missed;
}

/** Все объекты `properties`, в которых может лежать поле этого узла. */
function propertyHolders(defs, node, seen = new Set(), depth = 0) {
	const holders = [];
	if (!node || typeof node !== 'object' || depth > 8) return holders;
	if (node.$ref) {
		const key = node.$ref.replace('#/$defs/', '');
		if (seen.has(key) || !defs[key]) return holders;
		seen.add(key);
		return propertyHolders(defs, defs[key], seen, depth + 1);
	}
	if (node.properties) holders.push(node.properties);
	for (const group of ['oneOf', 'allOf', 'anyOf']) {
		for (const part of node[group] ?? []) holders.push(...propertyHolders(defs, part, new Set(seen), depth + 1));
	}
	return holders;
}

// ─────────────────────────────────────────────────────────────────────────────
// Сборка
// ─────────────────────────────────────────────────────────────────────────────

async function main() {
	console.log(`→ схема: ${SCHEMA_URL}`);
	const schema = JSON.parse(await fetchText(SCHEMA_URL));
	const defs = schema.$defs ?? {};
	console.log(`  $defs: ${Object.keys(defs).length}`);

	console.log(`→ документация: SagerNet/sing-box@${DOCS_REF}`);
	const docs = await fetchDocs();
	console.log(`  файлов: ${Object.keys(docs).length}`);

	const stats = { applied: 0, unions: 0, overlay: 0 };
	const unmapped = [];

	// Страницы из shared/ идут последними: applyDescriptions не перетирает уже
	// проставленное, поэтому специфичное описание поля всегда побеждает общее.
	const ordered = Object.entries(docs).sort(
		([a], [b]) => Number(a.startsWith('shared/')) - Number(b.startsWith('shared/'))
	);

	for (const [file, md] of ordered) {
		const map = parseDoc(md);
		if (!Object.keys(map).length) continue;

		let targets = TARGETS[file];

		// inbound/<type>.md и outbound/<type>.md → вариант с совпадающим `type`.
		const variantMatch = file.match(/^(inbound|outbound)\/([a-z0-9_]+)\.md$/);
		if (!targets && variantMatch) {
			const [, kind, type] = variantMatch;
			if (type === 'index') {
				targets = [kind === 'inbound' ? 'Inbound' : 'Outbound'];
			} else {
				const holder = defs[kind === 'inbound' ? 'Inbound' : 'Outbound'];
				const variants = (holder?.oneOf ?? []).filter((v) => variantTypes(defs, v).includes(type));
				if (variants.length) {
					for (const variant of variants) applyDescriptions(variant, map, stats);
					continue;
				}
			}
		}

		if (!targets) {
			unmapped.push(file);
			continue;
		}
		for (const name of targets) {
			if (defs[name]) applyDescriptions(defs[name], map, stats);
		}
	}

	// Описания расставляем до union-трансформа: он копирует ссылки на те же объекты,
	// поэтому подписи попадают в объединение сами собой.
	addUnionProps(defs, schema, stats);

	const missedOverlay = applyOverlay(schema, stats);

	schema['x-vantage-box'] = {
		note: 'Сгенерировано scripts/gen-singbox-schema.mjs. Руками не править.',
		generated: new Date().toISOString().slice(0, 10),
		schemaSource: SCHEMA_URL,
		docsSource: `SagerNet/sing-box@${DOCS_REF}:${DOCS_PREFIX}`,
		// Схема отслеживает 1.14-dev, а приложение поддерживает 1.10.7–1.13.x
		// (src-tauri/src/clash/client.rs). Поэтому ошибки схемы в редакторе —
		// подсказка, а не запрет: сохранение гейтится только `sing-box check`.
		appliesToNewerThanSupported: true
	};

	fs.writeFileSync(OUT, JSON.stringify(schema));
	const kb = (fs.statSync(OUT).size / 1024).toFixed(0);

	console.log(`\n✓ ${path.relative(ROOT, OUT)} — ${kb} КБ`);
	console.log(`  описаний из документации: ${stats.applied}`);
	console.log(`  русских переопределений:  ${stats.overlay} из ${Object.keys(ruOverlay).length}`);
	console.log(`  oneOf-узлов расширено:    ${stats.unions}`);
	if (unmapped.length) console.log(`  без таблицы (пропущено):  ${unmapped.join(', ')}`);
	if (missedOverlay.length) console.log(`  ⚠ оверлей не лёг:        ${missedOverlay.join(', ')}`);
}

// Позволяем импортировать parseDoc из проверочного скрипта, не запуская генерацию.
if (import.meta.url === `file:///${process.argv[1].replace(/\\/g, '/')}`) {
	main().catch((err) => {
		console.error('✗', err.message);
		process.exit(1);
	});
}
