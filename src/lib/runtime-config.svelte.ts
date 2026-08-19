// Whether the read-only "running config" viewer is open. A store, not props,
// so the error alert in AlertStrip can open the viewer without drilling
// through +page.svelte → ServiceView.

class RuntimeConfigModalState {
	open = $state(false);

	// Arrow methods so `this` stays bound when passed as a callback (the modal
	// calls onclose() without a receiver).
	show = () => {
		this.open = true;
	};

	hide = () => {
		this.open = false;
	};
}

export const runtimeConfigModal = new RuntimeConfigModalState();