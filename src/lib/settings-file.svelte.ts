// Whether the "settings file" editor is open. A store, not props, so it can be
// opened from the Settings tab without drilling through local state.

class SettingsFileModalState {
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

export const settingsFileModal = new SettingsFileModalState();