// Русский оверлей для схемы sing-box.
//
// Официальная схема (sing-box.sagernet.org/schema.json) описаний не содержит вообще,
// поэтому подписи генератор берёт из англоязычной документации SagerNet. Здесь — ручные
// русские тексты для того, что правят чаще всего; они перекрывают сгенерированные.
//
// Формат ключа:
//   'RouteOptions.rules'  — свойство `rules` внутри $defs.RouteOptions
//   'Rule.rule_set'       — свойство `rule_set` внутри $defs.Rule (любого его варианта)
//   'inbound:tun.stack'   — свойство `stack` у варианта inbound-а с type: "tun"
//   'outbound:selector.outbounds'
//   '#.route'             — свойство корня конфига
//
// Читается только генератором (scripts/gen-singbox-schema.mjs), в рантайм не попадает.

export const ruOverlay = {
	// ── Корень конфига ────────────────────────────────────────────────────────
	'#.log': 'Настройки журналирования.',
	'#.dns': 'DNS-серверы, правила и стратегии разрешения имён.',
	'#.ntp': 'Встроенный NTP-клиент. Нужен протоколам, чувствительным к времени (VMess, TUIC).',
	'#.certificate': 'Хранилище TLS-сертификатов.',
	'#.endpoints': 'Endpoint-ы — точки, которые одновременно и входящие, и исходящие (WireGuard и подобное).',
	'#.inbounds': 'Входящие соединения: tun, mixed, socks, http и другие.',
	'#.outbounds':
		'Исходящие соединения и группы. Группы типа "selector" — это то, что Vantage Box показывает на дашборде.',
	'#.route': 'Правила маршрутизации: куда какой трафик отправлять.',
	'#.services': 'Встроенные сервисы (DERP, resolved и подобное).',
	'#.experimental': 'Экспериментальные возможности, включая Clash API.',

	// ── log ───────────────────────────────────────────────────────────────────
	'LogOptions.disabled': 'Полностью отключить логи.',
	'LogOptions.level': 'Уровень журналирования.',
	'LogOptions.output': 'Путь к файлу лога. Пусто — вывод в stderr.',
	'LogOptions.timestamp': 'Добавлять отметку времени.',

	// ── route ─────────────────────────────────────────────────────────────────
	'RouteOptions.rules': 'Правила маршрутизации. Применяются сверху вниз, побеждает первое совпавшее.',
	'RouteOptions.rule_set': 'Наборы правил (rule-set): списки доменов и подсетей, подключаемые файлом или по URL.',
	'RouteOptions.final': 'Тег outbound-а, куда идёт трафик, не совпавший ни с одним правилом.',
	'RouteOptions.auto_detect_interface':
		'Автоматически определять исходящий интерфейс. Для TUN-режима на Windows практически обязательно.',
	'RouteOptions.default_domain_resolver': 'DNS-сервер по умолчанию для разрешения имён при исходящих соединениях.',
	'RouteOptions.find_process': 'Определять процесс-источник соединения. Нужно для правил по process_name/process_path.',

	// ── route.rules[] ─────────────────────────────────────────────────────────
	'Rule.rule_set': 'Совпадение с rule-set — набором правил, объявленным в route.rule_set.',
	'Rule.action': 'Что сделать с совпавшим трафиком: route, reject, hijack-dns, sniff, resolve.',
	'Rule.outbound': 'Тег outbound-а, куда направить совпавший трафик.',
	'Rule.inbound': 'Совпадение по тегу inbound-а, откуда пришло соединение.',
	'Rule.domain': 'Совпадение по полному доменному имени.',
	'Rule.domain_suffix': 'Совпадение по суффиксу домена.',
	'Rule.domain_keyword': 'Совпадение по подстроке в домене.',
	'Rule.domain_regex': 'Совпадение по регулярному выражению для домена.',
	'Rule.ip_cidr': 'Совпадение по подсети назначения.',
	'Rule.ip_is_private': 'Совпадение с непубличными адресами (локальная сеть, loopback).',
	'Rule.source_ip_cidr': 'Совпадение по подсети источника.',
	'Rule.port': 'Совпадение по порту назначения.',
	'Rule.port_range': 'Совпадение по диапазону портов назначения.',
	'Rule.process_name': 'Совпадение по имени процесса-источника. Требует route.find_process.',
	'Rule.process_path': 'Совпадение по полному пути процесса-источника.',
	'Rule.network': 'Совпадение по типу трафика: tcp, udp или icmp.',
	'Rule.protocol': 'Совпадение по протоколу, определённому сниффером (http, tls, quic, dns).',
	'Rule.invert': 'Инвертировать результат совпадения.',
	'Rule.clash_mode': 'Совпадение по текущему режиму Clash API (Rule, Global, Direct).',
	'Rule.type': 'Тип правила: "default" — обычное, "logical" — объединяет вложенные правила через and/or.',
	'Rule.mode': 'Способ объединения вложенных правил у logical-правила: and или or.',
	'Rule.rules': 'Вложенные правила logical-правила.',

	// ── route.rule_set[] ──────────────────────────────────────────────────────
	'RuleSet.type': 'Откуда берётся набор: "inline" — прямо здесь, "local" — файл на диске, "remote" — по URL.',
	'RuleSet.tag': 'Уникальное имя набора. Именно его указывают в rule_set у правил.',
	'RuleSet.format': 'Формат набора: "source" (JSON) или "binary" (.srs).',
	'RuleSet.url': 'Адрес загрузки для type: "remote".',
	'RuleSet.http_client': 'Тег HTTP-клиента, через который качать набор. Пусто — через route.final.',
	'RuleSet.update_interval': 'Как часто обновлять удалённый набор, например "1d".',
	'RuleSet.path': 'Путь к файлу набора для type: "local".',

	// ── dns ───────────────────────────────────────────────────────────────────
	'DNS.servers': 'Список DNS-серверов.',
	'DNS.rules': 'Правила выбора DNS-сервера. Применяются сверху вниз.',
	'DNS.final': 'Тег DNS-сервера по умолчанию для запросов, не совпавших ни с одним правилом.',
	'DNS.strategy': 'Стратегия разрешения по умолчанию: prefer_ipv4, prefer_ipv6, ipv4_only, ipv6_only.',
	'DNS.disable_cache': 'Отключить кэш DNS-ответов.',
	'DNS.cache_capacity': 'Размер кэша DNS-ответов в записях.',
	'DNSServer.tag': 'Уникальное имя сервера. На него ссылаются правила и dns.final.',
	'DNSServer.detour': 'Тег outbound-а, через который отправлять запросы к этому серверу.',
	'DNSRule.server': 'Тег DNS-сервера, на который уйдёт совпавший запрос.',
	'DNSRule.rule_set': 'Совпадение с rule-set — набором правил, объявленным в route.rule_set.',

	// ── inbounds ──────────────────────────────────────────────────────────────
	'inbound:tun.stack': 'Сетевой стек TUN: system, gvisor или mixed. На Windows обычно gvisor.',
	'inbound:tun.auto_route': 'Автоматически настроить системную маршрутизацию на TUN-интерфейс.',
	'inbound:tun.strict_route': 'Жёсткая маршрутизация: не давать трафику утечь мимо туннеля.',
	'inbound:tun.address': 'Адреса самого TUN-интерфейса, например 172.19.0.1/30.',
	'inbound:tun.mtu': 'MTU интерфейса. По умолчанию 9000.',
	'inbound:mixed.listen': 'Адрес прослушивания. 127.0.0.1 — только локально, 0.0.0.0 — из сети.',
	'inbound:mixed.listen_port': 'Порт прослушивания.',
	'inbound:mixed.users': 'Список пользователей для авторизации. Пусто — без авторизации.',

	// ── outbounds ─────────────────────────────────────────────────────────────
	'outbound:selector.outbounds': 'Теги outbound-ов, между которыми переключается группа.',
	'outbound:selector.default': 'Тег outbound-а, выбранного изначально.',
	'outbound:selector.interrupt_exist_connections': 'Рвать текущие соединения при переключении.',
	'outbound:urltest.outbounds': 'Теги outbound-ов, среди которых замеряется задержка.',
	'outbound:urltest.url': 'Адрес для замера задержки.',
	'outbound:urltest.interval': 'Периодичность замера, например "3m".',
	'outbound:urltest.tolerance': 'На сколько миллисекунд новый кандидат должен быть быстрее, чтобы переключиться.',
	'outbound:direct.type': 'Прямое соединение, минуя прокси.',
	'outbound:block.type': 'Блокировка: соединение обрывается.',

	// ── experimental ──────────────────────────────────────────────────────────
	'ExperimentalOptions.cache_file':
		'Файл кэша. Помимо прочего хранит выбор selector-групп между перезапусками.',
	'ExperimentalOptions.clash_api':
		'Clash API — через него Vantage Box управляет рантаймом. При запуске сервиса эта секция подставляется автоматически в рантайм-копию конфига.',
	'CacheFileOptions.enabled': 'Включить файл кэша.',
	'CacheFileOptions.path': 'Путь к файлу кэша.',
	'CacheFileOptions.cache_id': 'Идентификатор профиля внутри файла кэша.',
	'CacheFileOptions.store_fakeip': 'Хранить сопоставления FakeIP между запусками.',
	'CacheFileOptions.store_dns': 'Хранить кэш DNS-ответов между запусками.',
	'CacheFileOptions.rdrc_timeout': 'Срок жизни кэша отклонённых DNS-ответов.',
	'ClashAPIOptions.external_controller': 'Адрес API. Должен быть loopback, например 127.0.0.1:9090.',
	'ClashAPIOptions.secret': 'Токен авторизации API.',
	'ClashAPIOptions.default_mode': 'Режим по умолчанию: Rule, Global или Direct.'
};
