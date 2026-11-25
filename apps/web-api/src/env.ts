/**
 * web-api settings, read once from the process environment.
 *
 * Nothing here is ever logged: `BETTER_AUTH_SECRET` and
 * `HUMANIP_CONTROL_TOKEN` are secrets.
 */
export interface WebApiEnv {
	/** Bind address. Use 0.0.0.0 inside a container. */
	host: string;
	port: number;
	databasePath: string;
	betterAuthSecret: string;
	/** Origin of the console the browser uses, e.g. `http://localhost:1420`. */
	publicUrl: string;
	/** Base URL of `appctl serve`, e.g. `http://127.0.0.1:8787`. */
	controlUrl: string;
	controlToken: string;
	gatewayHost: string;
	socksPort: number;
	httpPort: number;
	/** Require a verified email before sign-in. Needs a mail sender; see docs/self-host.md. */
	requireEmailVerification: boolean;
}

function intVar(
	src: Record<string, string | undefined>,
	key: string,
	fallback: number,
): number {
	const raw = src[key];
	if (raw === undefined || raw.trim() === "") return fallback;
	const n = Number(raw);
	if (!Number.isInteger(n) || n < 0 || n > 65535) {
		throw new Error(`${key} must be an integer port number`);
	}
	return n;
}

function strVar(
	src: Record<string, string | undefined>,
	key: string,
	fallback: string,
): string {
	const raw = src[key]?.trim();
	return raw ? raw : fallback;
}

/** Parse and validate settings. Throws with a message naming the bad variable. */
export function loadEnv(
	src: Record<string, string | undefined> = process.env,
): WebApiEnv {
	const secret = src.BETTER_AUTH_SECRET ?? "";
	if (secret.length < 32) {
		throw new Error(
			"BETTER_AUTH_SECRET must be set to at least 32 characters (try: openssl rand -hex 32)",
		);
	}
	const publicUrl = strVar(src, "PUBLIC_URL", "http://localhost:1420").replace(
		/\/+$/,
		"",
	);
	try {
		new URL(publicUrl);
	} catch {
		throw new Error("PUBLIC_URL must be an absolute URL");
	}
	return {
		host: strVar(src, "HOST", "127.0.0.1"),
		port: intVar(src, "PORT", 3000),
		databasePath: strVar(src, "DATABASE_PATH", "./data/auth.db"),
		betterAuthSecret: secret,
		publicUrl,
		controlUrl: strVar(src, "CONTROL_URL", "http://127.0.0.1:8787").replace(
			/\/+$/,
			"",
		),
		controlToken: src.HUMANIP_CONTROL_TOKEN ?? "",
		gatewayHost: strVar(src, "GATEWAY_HOST", "localhost"),
		socksPort: intVar(src, "SOCKS_PORT", 1080),
		httpPort: intVar(src, "HTTP_PORT", 8080),
		requireEmailVerification: src.REQUIRE_EMAIL_VERIFICATION === "1",
	};
}
