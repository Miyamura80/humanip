/**
 * Wire the web-api: better-auth at `/api/auth/*`, app routes at `/app/api/*`.
 *
 * ```
 * request -> body limit -> request log (method, path, status only)
 *   /api/auth/*  -> CORS(PUBLIC_URL) -> better-auth handler
 *   /app/api/*   -> CORS(PUBLIC_URL) -> CSRF origin check -> session -> route
 * ```
 */
import { Database } from "bun:sqlite";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { Hono } from "hono";
import { bodyLimit } from "hono/body-limit";
import { cors } from "hono/cors";
import { AccountLinks } from "./accounts";
import {
	authOptions,
	createAuth,
	migrate,
	type SendVerificationEmail,
} from "./auth";
import { ControlClient, type ControlClientOptions } from "./control";
import type { WebApiEnv } from "./env";
import { appRoutes } from "./routes";

/** Largest accepted request body. Every route takes small JSON. */
const MAX_BODY_BYTES = 64 * 1024;

interface AppDeps {
	/** Override the SQLite handle (tests pass `new Database(":memory:")`). */
	db?: Database;
	/** Control client tuning (tests shorten the retry backoff). */
	control?: Partial<ControlClientOptions>;
	sendVerificationEmail?: SendVerificationEmail;
	/** Log one line per request. Off in tests. */
	logRequests?: boolean;
}

function openDb(path: string): Database {
	if (path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
	const db = new Database(path, { create: true });
	db.run("PRAGMA journal_mode = WAL");
	db.run("PRAGMA foreign_keys = ON");
	return db;
}

export async function createApp(env: WebApiEnv, deps: AppDeps = {}) {
	const db = deps.db ?? openDb(env.databasePath);
	const control = new ControlClient({
		baseUrl: env.controlUrl,
		token: env.controlToken,
		...deps.control,
	});
	const links = new AccountLinks(db, control);
	const options = authOptions(env, db, links, deps.sendVerificationEmail);
	await migrate(options);
	const auth = createAuth(options);

	const app = new Hono();
	app.use(
		"*",
		bodyLimit({
			maxSize: MAX_BODY_BYTES,
			onError: (c) => c.json({ error: "request body too large" }, 413),
		}),
	);
	if (deps.logRequests) {
		app.use("*", async (c, next) => {
			const start = performance.now();
			await next();
			// Path only: query strings and bodies are never logged.
			const ms = Math.round(performance.now() - start);
			console.log(`${c.req.method} ${c.req.path} ${c.res.status} ${ms}ms`);
		});
	}
	const corsMw = cors({
		origin: env.publicUrl,
		credentials: true,
		allowMethods: ["GET", "POST", "OPTIONS"],
		allowHeaders: ["content-type"],
		maxAge: 600,
	});
	app.use("/api/auth/*", corsMw);
	app.use("/app/api/*", corsMw);
	// Mutations must come from the console origin. hono/csrf only checks form
	// content types, so check Origin and Sec-Fetch-Site directly; JSON from
	// another site is also blocked by the CORS preflight above.
	app.use("/app/api/*", async (c, next) => {
		const m = c.req.method;
		if (m !== "GET" && m !== "HEAD" && m !== "OPTIONS") {
			const origin = c.req.header("origin");
			const site = c.req.header("sec-fetch-site");
			if ((origin && origin !== env.publicUrl) || site === "cross-site") {
				return c.json({ error: "cross-origin request rejected" }, 403);
			}
		}
		await next();
	});

	app.on(["GET", "POST"], "/api/auth/*", (c) => auth.handler(c.req.raw));
	app.route("/app/api", appRoutes(env, auth, links, control));
	app.get("/healthz", (c) => c.json({ ok: true }));
	app.notFound((c) => c.json({ error: "not found" }, 404));
	app.onError((err, c) => {
		console.error("web-api: unhandled error", err.name);
		return c.json({ error: "internal error" }, 500);
	});

	return { app, auth, db, links };
}
