// `useLive`: live backend data when it answers, demo data when it does not.
// Every mounted hook reports to a tiny store so the top nav can say which
// sources on the current screen are live.

import {
	useEffect,
	useId,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import type { Result } from "./types";

type Source = "live" | "demo";

interface Probe {
	label: string;
	source: Source;
	reason?: string;
}

// ─── Store ────────────────────────────────────────────────────────────────

const probes = new Map<string, Probe>();
let snapshot: Probe[] = [];
let epoch = 0;
const listeners = new Set<() => void>();

const emit = () => {
	snapshot = [...probes.values()];
	for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
	listeners.add(l);
	return () => {
		listeners.delete(l);
	};
};

/** Sources reported by the hooks mounted right now. */
export const useProbes = () => useSyncExternalStore(subscribe, () => snapshot);

/** Re-run every live hook, e.g. after the admin token changes. */
export function refetchAll(): void {
	epoch += 1;
	emit();
}
const useEpoch = () => useSyncExternalStore(subscribe, () => epoch);

// ─── Hook ─────────────────────────────────────────────────────────────────

interface Live<T> {
	data: T;
	source: Source;
	/** Why the source is demo, when it is. */
	reason?: string;
}

/**
 * Run `load` whenever `key` changes (debounced) and return its data, or
 * `fallback` with `source: "demo"` until it succeeds or after it fails.
 * A previous live answer stays on screen while the next one loads.
 */
export function useLive<T>(
	label: string,
	load: () => Promise<Result<T>>,
	fallback: T,
	key = "",
): Live<T> {
	const id = useId();
	const run = useEpoch();
	const loadRef = useRef(load);
	loadRef.current = load;
	const [state, setState] = useState<{ live?: T; reason: string }>({
		reason: "connecting",
	});

	useEffect(() => {
		let cancelled = false;
		// `run` and `key` are the triggers; read them so the effect re-runs.
		const delay = run >= 0 && key ? 220 : 0;
		const t = setTimeout(async () => {
			const r = await loadRef.current();
			if (cancelled) return;
			setState(r.ok ? { live: r.data, reason: "" } : { reason: r.reason });
		}, delay);
		return () => {
			cancelled = true;
			clearTimeout(t);
		};
	}, [key, run]);

	const live = state.live !== undefined;
	useEffect(() => {
		probes.set(id, {
			label,
			source: live ? "live" : "demo",
			reason: live ? undefined : state.reason,
		});
		emit();
	}, [id, label, live, state.reason]);
	useEffect(
		() => () => {
			probes.delete(id);
			emit();
		},
		[id],
	);

	return state.live !== undefined
		? { data: state.live, source: "live" }
		: { data: fallback, source: "demo", reason: state.reason };
}
