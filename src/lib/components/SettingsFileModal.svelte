<script lang="ts">
	import FileEditorModal from '$lib/components/FileEditorModal.svelte';
	import { api } from '$lib/api';
	import { m } from '$lib/paraglide/messages.js';
	import { settingsFileModal } from '$lib/settings-file.svelte';
	import { settingsSchema } from '$lib/settings-schema';

	async function save(content: string) {
		await api.writeSettingsFile(content);
	}
</script>

<FileEditorModal
	open={settingsFileModal.open}
	onclose={settingsFileModal.hide}
	title={m.settings_file_section}
	hint={m.settings_help_file}
	load={api.readSettingsFile}
	onsave={save}
	schema={settingsSchema}
/>