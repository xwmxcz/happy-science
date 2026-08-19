// Public product identity shared by the desktop UI and SDK-facing integrations.
// Native bundle metadata remains in tauri.conf.json because Tauri consumes it
// before TypeScript is built.

export const PRODUCT_NAME = "Happy Science";
export const PRODUCT_SLUG = "happy-science";

// Keep release checks off until Happy Science has its own public repository.
// Pointing a rebranded build at the upstream Open Science feed would advertise
// unrelated installers to users.
export const UPDATE_REPOSITORY: string | null = null;
