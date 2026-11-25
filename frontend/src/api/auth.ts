// better-auth client. The web-api mounts better-auth at `/api/auth/*` on the
// same origin as the console (Vite proxies it in dev), and keeps the session
// in an HTTP-only cookie, so nothing here stores a token.

import { createAuthClient } from "better-auth/react";

const authClient = createAuthClient({ baseURL: window.location.origin });

/** Current session; `data` is null when signed out. */
export const useSession = authClient.useSession;

type AuthResult = { ok: true } | { ok: false; message: string };
type AuthError = { message?: string; status?: number } | null;

/** Turn a better-auth `{error}` into one readable line. */
function outcome(error: AuthError): AuthResult {
	if (!error) return { ok: true };
	if (error.message) return { ok: false, message: error.message };
	const status = error.status ?? 0;
	if (status === 0 || status >= 500)
		return {
			ok: false,
			message: "The sign-in service is unavailable. Try again in a moment.",
		};
	return { ok: false, message: `Request failed (HTTP ${status})` };
}

async function guarded(
	run: () => Promise<{ error: unknown }>,
): Promise<AuthResult> {
	try {
		const { error } = await run();
		return outcome(error as AuthError);
	} catch {
		return { ok: false, message: "Could not reach the sign-in service." };
	}
}

export const signUp = (input: {
	email: string;
	password: string;
	name: string;
}) => guarded(() => authClient.signUp.email(input));

export const signIn = (input: { email: string; password: string }) =>
	guarded(() => authClient.signIn.email(input));

export const signOut = () => guarded(() => authClient.signOut());
