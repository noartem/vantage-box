// Manual overlay for the sing-box schema.
//
// The official schema (sing-box.sagernet.org/schema.json) contains no descriptions
// at all, so the generator pulls signatures from the SagerNet English-language docs.
// Here are manual texts for the things edited most often; they override the
// generated ones.
//
// Key format:
//   'RouteOptions.rules'  — the `rules` property inside $defs.RouteOptions
//   'Rule.rule_set'       — the `rule_set` property inside $defs.Rule (any variant)
//   'inbound:tun.stack'   — the `stack` property of the inbound variant with type: "tun"
//   'outbound:selector.outbounds'
//   '#.route'             — a property of the config root
//
// Read only by the generator (scripts/gen-singbox-schema.mjs); never reaches runtime.

export const ruOverlay = {
	// ── Config root ────────────────────────────────────────────────────────────
	'#.log': 'Logging settings.',
	'#.dns': 'DNS servers, rules, and name resolution strategies.',
	'#.ntp': 'Built-in NTP client. Needed by time-sensitive protocols (VMess, TUIC).',
	'#.certificate': 'TLS certificate store.',
	'#.endpoints': 'Endpoints — points that are simultaneously inbound and outbound (WireGuard and similar).',
	'#.inbounds': 'Inbound connections: tun, mixed, socks, http, and others.',
	'#.outbounds':
		'Outbound connections and groups. "selector"-type groups are what Vantage Box shows on the dashboard.',
	'#.route': 'Routing rules: where to send which traffic.',
	'#.services': 'Built-in services (DERP, resolved, and similar).',
	'#.experimental': 'Experimental features, including the Clash API.',

	// ── log ───────────────────────────────────────────────────────────────────
	'LogOptions.disabled': 'Disable logging entirely.',
	'LogOptions.level': 'Logging level.',
	'LogOptions.output': 'Path to the log file. Empty — output to stderr.',
	'LogOptions.timestamp': 'Add a timestamp.',

	// ── route ─────────────────────────────────────────────────────────────────
	'RouteOptions.rules': 'Routing rules. Applied top to bottom; the first match wins.',
	'RouteOptions.rule_set': 'Rule sets: lists of domains and subnets, included by file or URL.',
	'RouteOptions.final': 'The outbound tag for traffic that matched no rule.',
	'RouteOptions.auto_detect_interface':
		'Auto-detect the outbound interface. Practically required for TUN mode on Windows.',
	'RouteOptions.default_domain_resolver': 'Default DNS server for resolving names on outbound connections.',
	'RouteOptions.find_process': 'Detect the source process of a connection. Needed for process_name/process_path rules.',

	// ── route.rules[] ─────────────────────────────────────────────────────────
	'Rule.rule_set': 'Match against a rule set declared in route.rule_set.',
	'Rule.action': 'What to do with matched traffic: route, reject, hijack-dns, sniff, resolve.',
	'Rule.outbound': 'The outbound tag to send matched traffic to.',
	'Rule.inbound': 'Match by the inbound tag the connection came in through.',
	'Rule.domain': 'Match by the full domain name.',
	'Rule.domain_suffix': 'Match by a domain suffix.',
	'Rule.domain_keyword': 'Match by a substring in the domain.',
	'Rule.domain_regex': 'Match by a regular expression for the domain.',
	'Rule.ip_cidr': 'Match by the destination subnet.',
	'Rule.ip_is_private': 'Match non-public addresses (local network, loopback).',
	'Rule.source_ip_cidr': 'Match by the source subnet.',
	'Rule.port': 'Match by the destination port.',
	'Rule.port_range': 'Match by a destination port range.',
	'Rule.process_name': 'Match by the source process name. Requires route.find_process.',
	'Rule.process_path': 'Match by the full path of the source process.',
	'Rule.network': 'Match by traffic type: tcp, udp, or icmp.',
	'Rule.protocol': 'Match by the protocol detected by the sniffer (http, tls, quic, dns).',
	'Rule.invert': 'Invert the match result.',
	'Rule.clash_mode': 'Match by the current Clash API mode (Rule, Global, Direct).',
	'Rule.type': 'Rule type: "default" — ordinary, "logical" — combines nested rules with and/or.',
	'Rule.mode': 'How nested rules are combined in a logical rule: and or or.',
	'Rule.rules': 'Nested rules of a logical rule.',

	// ── route.rule_set[] ──────────────────────────────────────────────────────
	'RuleSet.type': 'Where the set comes from: "inline" — right here, "local" — a file on disk, "remote" — by URL.',
	'RuleSet.tag': 'Unique set name. This is what rules refer to in rule_set.',
	'RuleSet.format': 'Set format: "source" (JSON) or "binary" (.srs).',
	'RuleSet.url': 'Download URL for type: "remote".',
	'RuleSet.http_client': 'The HTTP client tag to download the set through. Empty — via route.final.',
	'RuleSet.update_interval': 'How often to update a remote set, e.g. "1d".',
	'RuleSet.path': 'Path to the set file for type: "local".',

	// ── dns ───────────────────────────────────────────────────────────────────
	'DNS.servers': 'List of DNS servers.',
	'DNS.rules': 'DNS server selection rules. Applied top to bottom.',
	'DNS.final': 'The default DNS server tag for queries that matched no rule.',
	'DNS.strategy': 'Default resolution strategy: prefer_ipv4, prefer_ipv6, ipv4_only, ipv6_only.',
	'DNS.disable_cache': 'Disable the DNS response cache.',
	'DNS.cache_capacity': 'DNS response cache size in entries.',
	'DNSServer.tag': 'Unique server name. Rules and dns.final refer to it.',
	'DNSServer.detour': 'The outbound tag to send queries to this server through.',
	'DNSRule.server': 'The DNS server tag the matched query goes to.',
	'DNSRule.rule_set': 'Match against a rule set declared in route.rule_set.',

	// ── inbounds ──────────────────────────────────────────────────────────────
	'inbound:tun.stack': 'TUN network stack: system, gvisor, or mixed. Usually gvisor on Windows.',
	'inbound:tun.auto_route': 'Automatically point system routing at the TUN interface.',
	'inbound:tun.strict_route': 'Strict routing: keep traffic from leaking past the tunnel.',
	'inbound:tun.address': 'Addresses of the TUN interface itself, e.g. 172.19.0.1/30.',
	'inbound:tun.mtu': 'Interface MTU. Default 9000.',
	'inbound:mixed.listen': 'Listen address. 127.0.0.1 — local only, 0.0.0.0 — from the network.',
	'inbound:mixed.listen_port': 'Listen port.',
	'inbound:mixed.users': 'List of users for authentication. Empty — no auth.',

	// ── outbounds ─────────────────────────────────────────────────────────────
	'outbound:selector.outbounds': 'Outbound tags the group switches between.',
	'outbound:selector.default': 'The outbound tag selected initially.',
	'outbound:selector.interrupt_exist_connections': 'Drop current connections on switch.',
	'outbound:urltest.outbounds': 'Outbound tags among which latency is measured.',
	'outbound:urltest.url': 'URL for latency measurement.',
	'outbound:urltest.interval': 'Measurement interval, e.g. "3m".',
	'outbound:urltest.tolerance': 'How many milliseconds faster a new candidate must be to switch.',
	'outbound:direct.type': 'Direct connection, bypassing the proxy.',
	'outbound:block.type': 'Block: the connection is dropped.',

	// ── experimental ──────────────────────────────────────────────────────────
	'ExperimentalOptions.cache_file':
		'Cache file. Among other things, stores selector-group choices across restarts.',
	'ExperimentalOptions.clash_api':
		'Clash API — Vantage Box controls the runtime through it. On service start this section is injected into the runtime config copy automatically.',
	'CacheFileOptions.enabled': 'Enable the cache file.',
	'CacheFileOptions.path': 'Path to the cache file.',
	'CacheFileOptions.cache_id': 'Profile identifier inside the cache file.',
	'CacheFileOptions.store_fakeip': 'Persist FakeIP mappings across launches.',
	'CacheFileOptions.store_dns': 'Persist the DNS response cache across launches.',
	'CacheFileOptions.rdrc_timeout': 'TTL of the rejected DNS response cache.',
	'ClashAPIOptions.external_controller': 'API address. Must be loopback, e.g. 127.0.0.1:9090.',
	'ClashAPIOptions.secret': 'API authorization token.',
	'ClashAPIOptions.default_mode': 'Default mode: Rule, Global, or Direct.'
};