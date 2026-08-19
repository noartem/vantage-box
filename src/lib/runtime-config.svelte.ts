// Whether the read-only "running config" viewer is open. A store, not props,
// so the error alert in AlertStrip can open the viewer without drilling
// through +page.svelte → ServiceView.

class RuntimeConfigModalState {
	open = $state(false);

	show() {
		this.open = true;
	}

	hide() {
		this.open = false;
	}
}

export const runtimeConfigModal = new RuntimeConfigModalState();