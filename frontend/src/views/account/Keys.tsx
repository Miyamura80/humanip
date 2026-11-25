// API keys: list, create (secret shown once), rotate (new secret shown
// once) and revoke, each destructive step behind a confirm.

import { type FormEvent, useState } from "react";
import { type ApiKey, createKey, revokeKey, rotateKey } from "../../api/app";
import { Dialog, SecretReveal } from "../../components/dialog";
import { Mark } from "../../components/icons";
import { usd } from "../../lib/policy";

type Pending =
	| { kind: "new" }
	| { kind: "secret"; title: string; secret: string }
	| { kind: "rotate" | "revoke"; key: ApiKey };

export function KeysPanel({
	keys,
	loading,
	error,
	onChange,
}: {
	keys: ApiKey[];
	loading: boolean;
	error?: string;
	onChange: () => void;
}) {
	const [pending, setPending] = useState<Pending>();
	const close = () => setPending(undefined);

	return (
		<section className="glass step">
			<div className="section-head spread">
				<div className="row">
					<Mark name="shield-user" size={20} />
					<h2>API keys</h2>
				</div>
				<button
					type="button"
					className="btn btn-small btn-primary"
					onClick={() => setPending({ kind: "new" })}
				>
					New key
				</button>
			</div>

			{error && <p className="form-error">Could not load keys: {error}</p>}
			{loading && !keys.length && <p className="faint">Loading keys...</p>}
			{!loading && !error && !keys.length && (
				<div className="empty">
					<Mark name="robot" size={28} />
					<span>
						No keys yet. Create one to point a proxy client at humanip.
					</span>
				</div>
			)}
			{keys.length > 0 && (
				<ul className="key-list">
					{keys.map((k) => (
						<li key={k.key_id} className={k.revoked ? "revoked" : ""}>
							<div className="key-name">
								<b>{k.name}</b>
								<span className="mono faint">{k.key_id}</span>
							</div>
							<span className={`pill ${k.revoked ? "pill-bad" : "pill-good"}`}>
								{k.revoked ? "Revoked" : "Active"}
							</span>
							<span className="muted key-cap">
								{k.monthly_cap_usd === null
									? "No monthly cap"
									: `${usd(k.monthly_cap_usd, 0)} / month`}
							</span>
							<div className="row key-actions">
								<button
									type="button"
									className="btn btn-small"
									disabled={k.revoked}
									onClick={() => setPending({ kind: "rotate", key: k })}
								>
									Rotate
								</button>
								<button
									type="button"
									className="btn btn-small btn-danger"
									disabled={k.revoked}
									onClick={() => setPending({ kind: "revoke", key: k })}
								>
									Revoke
								</button>
							</div>
						</li>
					))}
				</ul>
			)}

			{pending?.kind === "new" && (
				<NewKeyDialog
					onClose={close}
					onCreated={(name, secret) => {
						onChange();
						setPending({ kind: "secret", title: `Key "${name}"`, secret });
					}}
				/>
			)}
			{pending?.kind === "secret" && (
				<Dialog title={pending.title} onClose={close}>
					<SecretReveal secret={pending.secret} onDone={close} />
				</Dialog>
			)}
			{(pending?.kind === "rotate" || pending?.kind === "revoke") && (
				<ConfirmDialog
					action={pending.kind}
					apiKey={pending.key}
					onClose={close}
					onDone={(secret) => {
						onChange();
						if (secret)
							setPending({
								kind: "secret",
								title: `New secret for "${pending.key.name}"`,
								secret,
							});
						else close();
					}}
				/>
			)}
		</section>
	);
}

function NewKeyDialog({
	onClose,
	onCreated,
}: {
	onClose: () => void;
	onCreated: (name: string, secret: string) => void;
}) {
	const [name, setName] = useState("");
	const [cap, setCap] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const capNum = cap.trim() === "" ? null : Number(cap);
	const capOk = capNum === null || (capNum > 0 && capNum <= 100000);
	const nameOk = name.trim().length >= 1 && name.trim().length <= 64;

	const submit = async (e: FormEvent) => {
		e.preventDefault();
		if (!nameOk || !capOk) return;
		setBusy(true);
		setError("");
		const r = await createKey(name.trim(), capNum);
		setBusy(false);
		if (r.ok) onCreated(r.data.key.name, r.data.secret);
		else setError(r.reason);
	};

	return (
		<Dialog title="New API key" onClose={onClose}>
			<form className="stack" style={{ gap: 14 }} onSubmit={submit}>
				<label className="field" htmlFor="key-name">
					<span className="field-label">Name</span>
					<input
						id="key-name"
						className="input"
						maxLength={64}
						placeholder="e.g. browser-agent"
						value={name}
						onChange={(e) => setName(e.target.value)}
					/>
				</label>
				<label className="field" htmlFor="key-cap">
					<span className="field-label">Monthly cap, USD (optional)</span>
					<input
						id="key-cap"
						className="input mono"
						type="number"
						min={1}
						max={100000}
						placeholder="No cap"
						value={cap}
						onChange={(e) => setCap(e.target.value)}
					/>
				</label>
				{!capOk && (
					<p className="form-error">
						A cap must be above $0 and at most $100,000.
					</p>
				)}
				{error && (
					<p className="form-error" role="alert">
						{error}
					</p>
				)}
				<button
					type="submit"
					className="btn btn-primary"
					disabled={!nameOk || !capOk || busy}
				>
					{busy ? "Creating..." : "Create key"}
				</button>
			</form>
		</Dialog>
	);
}

const COPY = {
	rotate: {
		title: "Rotate key",
		body: "The current secret stops working right away. Clients need the new one.",
		cta: "Rotate",
	},
	revoke: {
		title: "Revoke key",
		body: "Clients using this key are cut off. This can't be undone.",
		cta: "Revoke",
	},
};

function ConfirmDialog({
	action,
	apiKey,
	onClose,
	onDone,
}: {
	action: "rotate" | "revoke";
	apiKey: ApiKey;
	onClose: () => void;
	onDone: (secret?: string) => void;
}) {
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const text = COPY[action];
	const run = async () => {
		setBusy(true);
		setError("");
		if (action === "rotate") {
			const r = await rotateKey(apiKey.key_id);
			setBusy(false);
			if (r.ok) onDone(r.data.secret);
			else setError(r.reason);
		} else {
			const r = await revokeKey(apiKey.key_id);
			setBusy(false);
			if (r.ok) onDone();
			else setError(r.reason);
		}
	};
	return (
		<Dialog title={`${text.title} "${apiKey.name}"?`} onClose={onClose}>
			<p className="muted" style={{ marginTop: 0 }}>
				{text.body}
			</p>
			{error && (
				<p className="form-error" role="alert">
					{error}
				</p>
			)}
			<div
				className="row"
				style={{ justifyContent: "flex-end", marginTop: 18 }}
			>
				<button type="button" className="btn" onClick={onClose}>
					Cancel
				</button>
				<button
					type="button"
					className={`btn ${action === "revoke" ? "btn-danger" : "btn-primary"}`}
					disabled={busy}
					onClick={run}
				>
					{busy ? "Working..." : text.cta}
				</button>
			</div>
		</Dialog>
	);
}
