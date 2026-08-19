<script lang="ts">
	import FileEditorModal from '$lib/components/FileEditorModal.svelte';
	import { api } from '$lib/api';
	import { m } from '$lib/paraglide/messages.js';
	import { runtimeConfigModal } from '$lib/runtime-config.svelte';
</script>

<FileEditorModal
	open={runtimeConfigModal.open}
	onclose={runtimeConfigModal.hide}
	title={m.runtime_config_title}
	hint={m.runtime_config_hint}
	load={api.readRuntimeConfig}
	readOnly
/>