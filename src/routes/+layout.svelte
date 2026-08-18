<script lang="ts">
	import '../app.css';
	import { applyLocale } from '$lib/i18n.svelte';

	// Resolve the UI language before anything renders, so there is no flash of the
	// base locale. Must run before the shell (+page.svelte) and its views mount.
	applyLocale();

	let { children } = $props();
</script>

{@render children()}
