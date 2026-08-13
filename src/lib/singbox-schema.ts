import type { JSONSchema7 } from 'json-schema';

/**
 * Схема конфига sing-box для редактора.
 *
 * Намеренно нестрогая: описан корень и те секции, форма которых точно известна
 * (log, experimental). Внутри `additionalProperties` везде разрешены, поэтому
 * схема даёт автокомплит и подсказки, но никогда не помечает ошибкой валидный
 * конфиг с ключами, которых мы не перечислили. Настоящая валидация — за
 * `sing-box check`.
 */
export const singboxSchema: JSONSchema7 = {
	$schema: 'http://json-schema.org/draft-07/schema#',
	title: 'sing-box configuration',
	type: 'object',
	additionalProperties: true,
	properties: {
		log: {
			type: 'object',
			description: 'Настройки журналирования.',
			additionalProperties: true,
			properties: {
				disabled: { type: 'boolean', description: 'Полностью отключить логи.' },
				level: {
					type: 'string',
					enum: ['trace', 'debug', 'info', 'warn', 'error', 'fatal', 'panic'],
					description: 'Уровень журналирования.'
				},
				output: { type: 'string', description: 'Путь к файлу лога. Пусто — вывод в stderr.' },
				timestamp: { type: 'boolean', description: 'Добавлять отметку времени.' }
			}
		},
		dns: { type: 'object', description: 'DNS-серверы, правила и стратегии.', additionalProperties: true },
		ntp: { type: 'object', description: 'Встроенный NTP-клиент.', additionalProperties: true },
		certificate: { type: 'object', description: 'Хранилище TLS-сертификатов.', additionalProperties: true },
		endpoints: {
			type: 'array',
			description: 'Endpoint-и (WireGuard и подобное).',
			items: taggedItem('Тип endpoint-а.')
		},
		inbounds: {
			type: 'array',
			description: 'Входящие соединения: tun, mixed, socks, http и другие.',
			items: taggedItem('Тип inbound-а, например "tun" или "mixed".')
		},
		outbounds: {
			type: 'array',
			description:
				'Исходящие соединения и группы. Группы типа "selector" — это то, что Vantage Box показывает на дашборде.',
			items: taggedItem('Тип outbound-а, например "selector", "urltest", "direct".')
		},
		route: { type: 'object', description: 'Правила маршрутизации.', additionalProperties: true },
		services: { type: 'object', description: 'Встроенные сервисы.', additionalProperties: true },
		experimental: {
			type: 'object',
			description: 'Экспериментальные возможности, включая Clash API.',
			additionalProperties: true,
			properties: {
				cache_file: {
					type: 'object',
					description:
						'Файл кэша. Помимо прочего хранит выбор selector-групп между перезапусками.',
					additionalProperties: true,
					properties: {
						enabled: { type: 'boolean' },
						path: { type: 'string' },
						cache_id: { type: 'string' },
						store_fakeip: { type: 'boolean' },
						store_rdrc: { type: 'boolean' }
					}
				},
				clash_api: {
					type: 'object',
					description:
						'Clash API — через него Vantage Box управляет рантаймом. При запуске сервиса эта секция подставляется автоматически в рантайм-копию конфига.',
					additionalProperties: true,
					properties: {
						external_controller: {
							type: 'string',
							description: 'Адрес API. Должен быть loopback, например 127.0.0.1:9090.'
						},
						external_ui: { type: 'string' },
						external_ui_download_url: { type: 'string' },
						external_ui_download_detour: { type: 'string' },
						secret: { type: 'string', description: 'Токен авторизации API.' },
						default_mode: { type: 'string' },
						access_control_allow_origin: { type: 'array', items: { type: 'string' } },
						access_control_allow_private_network: { type: 'boolean' }
					}
				},
				v2ray_api: { type: 'object', additionalProperties: true }
			}
		}
	}
};

function taggedItem(typeDescription: string): JSONSchema7 {
	return {
		type: 'object',
		additionalProperties: true,
		properties: {
			type: { type: 'string', description: typeDescription },
			tag: {
				type: 'string',
				description: 'Уникальное имя. Именно теги видны в интерфейсе и в Clash API.'
			}
		}
	};
}
