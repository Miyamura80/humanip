import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The frontend is an optional visualization layer over the `appctl serve`
// HTTP API. `vite` is invoked with this directory as its root (see the
// `dev`/`build` scripts in the repo-root package.json).
//
// In development, `/api` (and `/healthz`) are proxied to `appctl serve` so the
// browser calls the same HTTP API it will hit in production, without CORS
// gymnastics. Override the target with `VITE_API_PROXY` if the server binds
// elsewhere.

// @ts-expect-error process is a nodejs global
const apiProxyTarget = process.env.VITE_API_PROXY || "http://127.0.0.1:8080";
// The gateway admin API (`/admin/v1/status`, `/admin/v1/usage`). The bearer
// token comes from `VITE_ADMIN_TOKEN` or the console's settings popover.
// @ts-expect-error process is a nodejs global
const adminProxyTarget =
	process.env.VITE_ADMIN_PROXY || "http://127.0.0.1:9090";

// https://vite.dev/config/
export default defineConfig({
	plugins: [react()],
	// Don't clear the terminal - keeps `appctl serve` logs visible alongside Vite.
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		proxy: {
			"/api": { target: apiProxyTarget, changeOrigin: true },
			"/healthz": { target: apiProxyTarget, changeOrigin: true },
			"/admin": { target: adminProxyTarget, changeOrigin: true },
		},
	},
});
