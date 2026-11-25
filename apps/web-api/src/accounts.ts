/**
 * user id -> humanip account id mapping.
 *
 * Strategy: the better-auth user-create hook tries to open a control account
 * right after the user row is written. If the control API is down the signup
 * still succeeds, and the account is created lazily (with retries) by the
 * first `/app/api/*` call that needs it. A user can therefore exist briefly
 * without a mapping, but no route ever acts without one: `ensure` either
 * returns an account id or throws, and routes turn that into 503.
 *
 * The mapping lives in its own table next to better-auth's tables, so the
 * account id never travels in the session payload and can never be set by a
 * client through better-auth's user update endpoints.
 */
import type { Database } from "bun:sqlite";
import type { ControlClient } from "./control";

export class AccountLinks {
	/** One in-flight creation per user, so concurrent requests do not race. */
	private readonly pending = new Map<string, Promise<string>>();

	constructor(
		private readonly db: Database,
		private readonly control: ControlClient,
	) {
		db.run(
			`CREATE TABLE IF NOT EXISTS humanip_account (
				user_id TEXT PRIMARY KEY NOT NULL,
				account_id TEXT NOT NULL,
				created_at INTEGER NOT NULL
			)`,
		);
	}

	get(userId: string): string | null {
		const row = this.db
			.query("SELECT account_id FROM humanip_account WHERE user_id = ?")
			.get(userId) as { account_id: string } | null;
		return row?.account_id ?? null;
	}

	/** Return the user's account id, creating the control account if needed. */
	ensure(userId: string, email: string): Promise<string> {
		const existing = this.get(userId);
		if (existing) return Promise.resolve(existing);
		const inflight = this.pending.get(userId);
		if (inflight) return inflight;
		const p = this.create(userId, email).finally(() => {
			this.pending.delete(userId);
		});
		this.pending.set(userId, p);
		return p;
	}

	private async create(userId: string, email: string): Promise<string> {
		const { account_id } = await this.control.createAccount(email);
		// First writer wins; a racing process that also created an account
		// leaves an empty, unlinked one behind, which is harmless.
		this.db
			.query(
				"INSERT OR IGNORE INTO humanip_account (user_id, account_id, created_at) VALUES (?, ?, ?)",
			)
			.run(userId, account_id, Math.floor(Date.now() / 1000));
		const linked = this.get(userId);
		if (!linked) throw new Error("account link was not stored");
		return linked;
	}
}
