// Small data hooks over the web-api client: a generic loader with explicit
// loading / error / ok states, and a shared `/app/api/me` store so the top
// nav and the Account page show the same balance.

import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { type GatewayConfig, getConfig, getMe, type Me } from "./app";
import type { Result } from "./types";

type Resource<T> =
	| { status: "loading"; data?: T }
	| { status: "error"; error: string; data?: T }
	| { status: "ok"; data: T };

/** Load once on mount and whenever `reload` is called. */
export function useResource<T>(load: () => Promise<Result<T>>) {
	const [state, setState] = useState<Resource<T>>({ status: "loading" });
	const [tick, setTick] = useState(0);
	const [loader] = useState(() => load);

	useEffect(() => {
		let cancelled = false;
		if (tick >= 0)
			loader().then((r) => {
				if (cancelled) return;
				setState((prev) =>
					r.ok
						? { status: "ok", data: r.data }
						: { status: "error", error: r.reason, data: prev.data },
				);
			});
		return () => {
			cancelled = true;
		};
	}, [loader, tick]);

	const reload = useCallback(() => setTick((t) => t + 1), []);
	return { ...state, reload };
}

// ─── /app/api/me ──────────────────────────────────────────────────────────

let me: Resource<Me> = { status: "loading" };
const listeners = new Set<() => void>();
const subscribe = (l: () => void) => {
	listeners.add(l);
	return () => {
		listeners.delete(l);
	};
};

const publish = (next: Resource<Me>) => {
	me = next;
	for (const l of listeners) l();
};
let inflight: Promise<Result<Me>> | undefined;

/** Fetch `/app/api/me` and publish it to every `useMe` caller. */
export function refreshMe(): Promise<Result<Me>> {
	inflight ??= getMe().then((r) => {
		inflight = undefined;
		publish(
			r.ok
				? { status: "ok", data: r.data }
				: { status: "error", error: r.reason, data: me.data },
		);
		return r;
	});
	return inflight;
}

/** Forget the cached user, e.g. on sign out. */
export const resetMe = () => publish({ status: "loading" });

/** The signed-in user and account; loads on first use. */
export function useMe(): Resource<Me> {
	const snap = useSyncExternalStore(subscribe, () => me);
	const empty = snap.status === "loading" && !snap.data;
	useEffect(() => {
		if (empty) refreshMe();
	}, [empty]);
	return snap;
}

// ─── /app/api/config ──────────────────────────────────────────────────────

let config: Promise<Result<GatewayConfig>> | undefined;

/** Gateway host and ports for connect snippets; null until known. */
export function useGatewayConfig(): GatewayConfig | null {
	const [cfg, setCfg] = useState<GatewayConfig | null>(null);
	useEffect(() => {
		let cancelled = false;
		config ??= getConfig();
		config.then((r) => {
			if (!r.ok) config = undefined;
			else if (!cancelled) setCfg(r.data);
		});
		return () => {
			cancelled = true;
		};
	}, []);
	return cfg;
}
