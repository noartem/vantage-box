// Generator of the sing-box config schema for the editor.
//
// Why a generator at all:
//   1. sing-box has an official schema (since 1.14.0-beta.2), but it has ZERO descriptions —
//      4805 nodes, not a single `description`. It provides autocomplete, but no hints.
//      Texts have to be assembled from the SagerNet docs (markdown, `#### field`).
//   2. The schema describes inbounds/outbounds/route.rules as `oneOf` variants with a `type`
//      discriminator. json-schema-library (which codemirror-json-schema builds on) returns a
//      bare `oneOf` with no `properties` at such a node — i.e. no hover hints
//      exactly where they matter most. Fixed by the union transform below.
//
// Run: task schema:update  (or npm run schema:update)
// Output: src/lib/singbox-schema.generated.json — committed to the repo so the build
// does not depend on the network.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ruOverlay } from './singbox-schema-overlay.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT = path.join(ROOT, 'src', 'lib', 'singbox-schema.generated.json');

const SCHEMA_URL = 'https://sing-box.sagernet.org/schema.json';
// The branch whose docs match the same version as the published schema.
const DOCS_REF = 'dev-next-wip';
const DOCS_PREFIX = 'docs/configuration/';

// ─────────────────────────────────────────────────────────────────────────────
// Loading
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
	if (!tree.tree) throw new Error(`failed to fetch repo tree: ${JSON.stringify(tree).slice(0, 200)}`);

	const files = tree.tree
		.map((f) => f.path)
		.filter((p) => p.startsWith(DOCS_PREFIX) && p.endsWith('.md') && !p.includes('.zh.'))
		.sort();

	const docs = {};
	// In small batches so we don't hammer raw.githubusercontent with a hundred parallel requests.
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
// Parsing the docs: `#### field_name` → description
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Extracts a "field name → description" map from a markdown page.
 * Accounts for SagerNet markup quirks:
 *   - `!!! question "Since sing-box 1.8.0"` → append to the description in italics;
 *   - `==Required==` → this is a marker, not text: record the flag and take the next paragraph;
 *   - `[text](url)` links collapse to text (you can't click a URL inside a tooltip anyway).
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
		// Any other heading closes the current field.
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
		if (/^\s{4}/.test(line)) continue; // admonition body
		if (/^===\s/.test(line)) continue; // tab switches
		if (/^==.+==\s*$/.test(line.trim())) {
			if (/required/i.test(line)) required = true;
			continue;
		}
		text.push(line);
	}

	// The first non-empty paragraph is the description.
	const body = text
		.join('\n')
		.split(/\n\s*\n/)
		.map((p) => p.trim())
		.find((p) => p.length > 0);

	let desc = (body ?? '')
		.replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // links → text
		.replace(/\s*\n\s*/g, ' ')
		.trim();

	if (required) desc = desc ? `**Required.** ${desc}` : '**Required.**';
	if (!desc && notes.length === 0) return null;
	return [desc, ...notes.map((n) => `*${n}*`)].filter(Boolean).join('\n\n');
}

// ─────────────────────────────────────────────────────────────────────────────
// Laying descriptions onto the schema
// ─────────────────────────────────────────────────────────────────────────────

// Tagged-union holders. In the official schema, the shared listener/dialer options are
// NOT factored into a separate $def but duplicated inside every variant (listen_port —
// 37 declarations, detour — 80). So we lay pages from shared/ onto these nodes whole:
// applyDescriptions will walk all the variants itself.
const UNION_HOLDERS = ['Inbound', 'Outbound', 'Endpoint', 'Service', 'DNSServer'];

// Rules with action "route"/"resolve" accept the same dialer options (bind_interface,
// tcp_fast_open, connect_timeout, etc.) — in the schema they are also duplicated inside.
const ACTION_HOLDERS = ['Rule', 'NestedRule', 'RuleAction', 'DNSRule', 'NestedDNSRule', 'DNSRuleAction'];

// Which docs file describes which $defs. No table needed for inbound/outbound:
// the file name matches the `type` value, so we find the right variant by it.
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

/** Variant properties with allOf/$ref expanded — same logic as in the union transform. */
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

/** The `type` values a tagged-union variant covers. */
function variantTypes(defs, variant) {
	const props = collectProps(defs, variant);
	const t = props.type;
	if (!t) return [];
	if (Array.isArray(t.enum)) return t.enum.filter(Boolean);
	if (t.const) return [t.const];
	return [];
}

/**
 * Places descriptions inside a subtree. Does not follow $refs — other $defs are described
 * by their own docs file, otherwise tun's texts would leak across all inbounds.
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
// Union transform: the whole point of this exercise
// ─────────────────────────────────────────────────────────────────────────────

/**
 * For every `oneOf` node, sets `properties` = the union of all variants' properties.
 * The `oneOf` itself stays untouched, so validation is not weakened: `oneOf` still
 * rejects invalid combinations, and `properties` gives the resolver something to
 * anchor hover hints to.
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
// Manual overlay
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
			// Look for the property in the node itself and in all its variants/parts.
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

/** All `properties` objects where this node's field may live. */
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
// Build
// ─────────────────────────────────────────────────────────────────────────────

async function main() {
	console.log(`→ schema: ${SCHEMA_URL}`);
	const schema = JSON.parse(await fetchText(SCHEMA_URL));
	const defs = schema.$defs ?? {};
	console.log(`  $defs: ${Object.keys(defs).length}`);

	console.log(`→ docs: SagerNet/sing-box@${DOCS_REF}`);
	const docs = await fetchDocs();
	console.log(`  files: ${Object.keys(docs).length}`);

	const stats = { applied: 0, unions: 0, overlay: 0 };
	const unmapped = [];

	// Pages from shared/ go last: applyDescriptions does not overwrite already-set
	// descriptions, so a field's specific description always wins over the shared one.
	const ordered = Object.entries(docs).sort(
		([a], [b]) => Number(a.startsWith('shared/')) - Number(b.startsWith('shared/'))
	);

	for (const [file, md] of ordered) {
		const map = parseDoc(md);
		if (!Object.keys(map).length) continue;

		let targets = TARGETS[file];

		// inbound/<type>.md and outbound/<type>.md → the variant with a matching `type`.
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

	// Lay descriptions before the union transform: it copies references to the same
	// objects, so signatures land in the union on their own.
	addUnionProps(defs, schema, stats);

	const missedOverlay = applyOverlay(schema, stats);

	schema['x-vantage-box'] = {
		note: 'Generated by scripts/gen-singbox-schema.mjs. Do not edit by hand.',
		generated: new Date().toISOString().slice(0, 10),
		schemaSource: SCHEMA_URL,
		docsSource: `SagerNet/sing-box@${DOCS_REF}:${DOCS_PREFIX}`,
		// The schema tracks 1.14-dev, while the app supports 1.10.7–1.13.x
		// (src-tauri/src/clash/client.rs). So schema errors in the editor are a
		// hint, not a block: saving is gated only by `sing-box check`.
		appliesToNewerThanSupported: true
	};

	fs.writeFileSync(OUT, JSON.stringify(schema));
	const kb = (fs.statSync(OUT).size / 1024).toFixed(0);

	console.log(`\n✓ ${path.relative(ROOT, OUT)} — ${kb} KB`);
	console.log(`  descriptions from docs:     ${stats.applied}`);
	console.log(`  manual overrides applied:    ${stats.overlay} of ${Object.keys(ruOverlay).length}`);
	console.log(`  oneOf nodes expanded:        ${stats.unions}`);
	if (unmapped.length) console.log(`  no table (skipped):           ${unmapped.join(', ')}`);
	if (missedOverlay.length) console.log(`  ⚠ overlay missed:            ${missedOverlay.join(', ')}`);
}

// Allow importing parseDoc from the verification script without running the generator.
if (import.meta.url === `file:///${process.argv[1].replace(/\\/g, '/')}`) {
	main().catch((err) => {
		console.error('✗', err.message);
		process.exit(1);
	});
}