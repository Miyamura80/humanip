/** Entry point: `bun run src/index.ts`. See docs/self-host.md for env vars. */
import { createApp } from "./app";
import { loadEnv } from "./env";

const env = loadEnv();
const { app } = await createApp(env, { logRequests: true });

if (!env.controlToken) {
	console.warn(
		"web-api: HUMANIP_CONTROL_TOKEN is empty; control calls will fail",
	);
}

const server = Bun.serve({
	hostname: env.host,
	port: env.port,
	fetch: app.fetch,
});
console.log(
	`web-api listening on http://${env.host}:${server.port} (public ${env.publicUrl}, control ${env.controlUrl})`,
);
