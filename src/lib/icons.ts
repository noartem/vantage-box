/**
 * Icons — only the `d` strings for <path>, viewBox 24x24, stroke 2, rounded caps
 * (lucide style). We do not pull in a library: there are fewer than thirty icons
 * we need, and any icon pack would add more to the bundle than this file weighs.
 *
 * Circles are written as arcs (`a r r 0 1 0 …`) rather than <circle>, so
 * Icon.svelte can render everything in a single loop over paths.
 */
export const icons = {
	// Tabs
	dashboard: ['m12 14 4-4', 'M3.34 19a10 10 0 1 1 17.32 0'],
	connections: ['M8 3 4 7l4 4', 'M4 7h16', 'm16 21 4-4-4-4', 'M20 17H4'],
	subscriptions: ['M20 16.2A4.5 4.5 0 0 0 17.5 8h-1.8A7 7 0 1 0 4 14.9', 'M12 12v9', 'm8 17 4 4 4-4'],
	config: [
		'M8 3H7a2 2 0 0 0-2 2v5a2 2 0 0 1-2 2 2 2 0 0 1 2 2v5c0 1.1.9 2 2 2h1',
		'M16 21h1a2 2 0 0 0 2-2v-5c0-1.1.9-2 2-2a2 2 0 0 1-2-2V5a2 2 0 0 0-2-2h-1'
	],
	logs: ['M8 6h13', 'M8 12h13', 'M8 18h13', 'M3 6h.01', 'M3 12h.01', 'M3 18h.01'],
	service: [
		'M4 3h16a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z',
		'M4 14h16a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2z',
		'M6 6.5h.01',
		'M6 17.5h.01'
	],
	settings: [
		'M20 7h-9',
		'M14 17H5',
		'M17 14a3 3 0 1 0 0 6 3 3 0 1 0 0-6',
		'M7 4a3 3 0 1 0 0 6 3 3 0 1 0 0-6'
	],

	// sing-box controls
	play: ['M7 4.5v15l13-7.5z'],
	stop: ['M7 6h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z'],
	restart: ['M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8', 'M21 3v5h-5'],
	pause: ['M6 4h3v16H6z', 'M15 4h3v16h-3z'],

	// Actions
	refresh: ['M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8', 'M21 3v5h-5'],
	search: ['M11 4a7 7 0 1 0 0 14 7 7 0 1 0 0-14', 'm21 21-4.3-4.3'],
	close: ['M18 6 6 18', 'm6 6 12 12'],
	plus: ['M12 5v14', 'M5 12h14'],
	trash: [
		'M3 6h18',
		'M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2',
		'M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6'
	],
	download: ['M12 3v12', 'm7 10 5 5 5-5', 'M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4'],
	copy: [
		'M9 9h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2V11a2 2 0 0 1 2-2z',
		'M5 15H4a2 2 0 0 1-2-2V3a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v1'
	],
	save: [
		'M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z',
		'M17 21v-8H7v8',
		'M7 3v5h8'
	],
	folder: [
		'm6 14 1.45-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.55 6a2 2 0 0 1-1.94 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2'
	],
	external: [
		'M15 3h6v6',
		'M10 14 21 3',
		'M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6'
	],
	book: [
		'M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z',
		'M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z'
	],
	zap: ['M13 2 3 14h9l-1 8 10-12h-9z'],
	check: ['M20 6 9 17l-5-5'],

	// Navigation and status
	chevronRight: ['m9 18 6-6-6-6'],
	chevronLeft: ['m15 18-6-6 6-6'],
	chevronDown: ['m6 9 6 6 6-6'],
	chevronUp: ['m18 15-6-6-6 6'],
	alert: ['m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3', 'M12 9v4', 'M12 17h.01'],
	info: ['M12 3a9 9 0 1 0 0 18 9 9 0 1 0 0-18', 'M12 16v-4', 'M12 8h.01'],
	sortAsc: ['M12 19V5', 'm5 12 7-7 7 7'],
	sortDesc: ['M12 5v14', 'm19 12-7 7-7-7']
} as const;

export type IconName = keyof typeof icons;
