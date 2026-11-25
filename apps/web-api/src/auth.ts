/**
 * better-auth setup: email + password, SQLite through `bun:sqlite`, sessions
 * in an HttpOnly SameSite=Lax cookie, built-in rate limiting always on.
 */
import type { Database } from "bun:sqlite";
import { type BetterAuthOptions, betterAuth } from "better-auth";
import { getMigrations } from "better-auth/db/migration";
import type { AccountLinks } from "./accounts";
import type { WebApiEnv } from "./env";

/** Sends a verification link. Required when email verification is on. */
export type SendVerificationEmail = (to: string, url: string) => Promise<void>;

export function authOptions(
	env: WebApiEnv,
	db: Database,
	links: AccountLinks,
	sendVerificationEmail?: SendVerificationEmail,
): BetterAuthOptions {
	if (env.requireEmailVerification && !sendVerificationEmail) {
		throw new Error(
			"REQUIRE_EMAIL_VERIFICATION=1 needs a mail sender: pass sendVerificationEmail to createApp (see docs/self-host.md)",
		);
	}
	return {
		appName: "humanip",
		baseURL: env.publicUrl,
		basePath: "/api/auth",
		secret: env.betterAuthSecret,
		trustedOrigins: [env.publicUrl],
		database: db,
		emailAndPassword: {
			enabled: true,
			requireEmailVerification: env.requireEmailVerification,
			minPasswordLength: 8,
			maxPasswordLength: 128,
		},
		emailVerification: sendVerificationEmail
			? {
					sendOnSignUp: true,
					sendVerificationEmail: async ({ user, url }) => {
						await sendVerificationEmail(user.email, url);
					},
				}
			: undefined,
		// better-auth only enables this in production by default; keep it on
		// everywhere so local runs behave like a deployment.
		rateLimit: { enabled: true, window: 60, max: 100 },
		advanced: {
			defaultCookieAttributes: { httpOnly: true, sameSite: "lax" },
		},
		logger: { level: "warn" },
		databaseHooks: {
			user: {
				create: {
					after: async (user) => {
						try {
							await links.ensure(user.id, user.email);
						} catch {
							// Control API down: the first /app/api call retries.
							console.warn(
								"web-api: account creation deferred (control API unavailable)",
							);
						}
					},
				},
			},
		},
	};
}

export function createAuth(options: BetterAuthOptions) {
	return betterAuth(options);
}

export type Auth = ReturnType<typeof createAuth>;

/** Create or update better-auth's tables. Safe to run on every start. */
export async function migrate(options: BetterAuthOptions): Promise<void> {
	const { runMigrations } = await getMigrations(options);
	await runMigrations();
}
