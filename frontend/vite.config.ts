import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The console talks to three backends, all same-origin through this dev
// proxy so cookies and fetch work without CORS (docs/product-api.md, ports):
//
//   /api/auth, /app  ->  web-api (Bun + better-auth)   VITE_WEB_API_PROXY
//   /api/v1, /healthz -> appctl serve                  VITE_API_PROXY
//   /admin           ->  gateway admin API             VITE_ADMIN_PROXY
//
// `vite` is invoked with this directory as its root (see the `dev`/`build`
// scripts in the repo-root package.json).

// @ts-expect-error process is a nodejs global
const env: Record<string, string | undefined> = process.env;
const webApiProxyTarget = env.VITE_WEB_API_PROXY || "http://127.0.0.1:3000";
const apiProxyTarget = env.VITE_API_PROXY || "http://127.0.0.1:8787";
// The gateway admin API (`/admin/v1/status`, `/admin/v1/usage`). The bearer
// token comes from `VITE_ADMIN_TOKEN` or the console's settings popover.
const adminProxyTarget = env.VITE_ADMIN_PROXY || "http://127.0.0.1:9090";

// https://vite.dev/config/
export default defineConfig({
	plugins: [react()],
	// Don't clear the terminal - keeps `appctl serve` logs visible alongside Vite.
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		proxy: {
			// Most specific first: `/api/auth` must not fall through to appctl.
			"/api/auth": { target: webApiProxyTarget, changeOrigin: false },
			"/app": { target: webApiProxyTarget, changeOrigin: false },
			"/api/v1": { target: apiProxyTarget, changeOrigin: true },
			"/healthz": { target: apiProxyTarget, changeOrigin: true },
			"/admin": { target: adminProxyTarget, changeOrigin: true },
		},
	},
});
