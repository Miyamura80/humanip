import { Database } from "bun:sqlite";
import { createApp } from "../src/app";
import { loadEnv } from "../src/env";

export const PUBLIC_URL = "http://localhost:1420";
export const TOKEN = "test-control-token";

let ipSeq = 1;

/** Parse a test response body without type ceremony. */
export const j = (r: Response): Promise<any> => r.json();

export async function makeApp(controlUrl: string) {
	const env = loadEnv({
		BETTER_AUTH_SECRET: "x".repeat(40),
		PUBLIC_URL,
		CONTROL_URL: controlUrl,
		HUMANIP_CONTROL_TOKEN: TOKEN,
		GATEWAY_HOST: "gw.test",
		SOCKS_PORT: "1081",
		HTTP_PORT: "8081",
	});
	const built = await createApp(env, {
		db: new Database(":memory:"),
		control: { backoffMs: 1, retries: 1, timeoutMs: 2000 },
	});
	const app = built.app;

	/** Each client gets its own IP so better-auth's rate limiter keys apart. */
	function client() {
		const ip = `10.0.0.${ipSeq++}`;
		let cookie = "";
		async function call(method: string, path: string, body?: unknown) {
			const headers: Record<string, string> = {
				origin: PUBLIC_URL,
				"x-forwarded-for": ip,
			};
			if (cookie) headers.cookie = cookie;
			if (body !== undefined) headers["content-type"] = "application/json";
			const res = await app.request(path, {
				method,
				headers,
				body: body === undefined ? undefined : JSON.stringify(body),
			});
			const set = res.headers.getSetCookie();
			if (set.length) {
				cookie = set.map((c) => c.split(";")[0]).join("; ");
			}
			return res;
		}
		return {
			call,
			get cookie() {
				return cookie;
			},
			async signUp(email: string, password = "correct-horse-battery") {
				return call("POST", "/api/auth/sign-up/email", {
					email,
					password,
					name: email.split("@")[0],
				});
			},
		};
	}

	return { app, client, links: built.links };
}
