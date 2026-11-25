/// <reference types="vite/client" />

interface ImportMetaEnv {
	/** Gateway admin bearer token for dev; the settings popover overrides it. */
	readonly VITE_ADMIN_TOKEN?: string;
}
