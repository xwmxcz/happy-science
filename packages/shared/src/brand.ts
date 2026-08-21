// Public product identity shared by the desktop UI and SDK-facing integrations.
// Native bundle metadata remains in tauri.conf.json because Tauri consumes it
// before TypeScript is built.

export const PRODUCT_NAME = "Happy Science";
export const PRODUCT_SLUG = "happy-science";

// Single owner for the public release feed. The desktop passes this repository
// to the native fetch bridge; the gateway client derives the GitHub API URL.
export const UPDATE_REPOSITORY = "xwmxcz/happy-science";
