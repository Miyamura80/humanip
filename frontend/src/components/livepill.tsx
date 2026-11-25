// Top-nav data source indicator ("Live" / "Demo data") and the small
// settings popover that holds the gateway admin token.

import { useState } from "react";
import { saveAdminToken, savedAdminToken } from "../api/client";
import { refetchAll, useProbes } from "../api/live";
import { Mark } from "./icons";

export function LivePill() {
	const probes = useProbes();
	if (!probes.length) return null;
	const live = probes.filter((p) => p.source === "live");
	const all = live.length === probes.length;
	const none = live.length === 0;
	const tip = probes
		.map((p) =>
			p.source === "live"
				? `${p.label}: live`
				: `${p.label}: offline (${p.reason}), showing demo data`,
		)
		.join("\n");
	return (
		<output
			className={`pill source-pill ${all ? "pill-good" : none ? "" : "pill-warn"}`}
			title={tip}
		>
			<span className={`source-dot${none ? "" : " on"}`} aria-hidden="true" />
			{all ? "Live" : none ? "Demo data" : "Partly live"}
		</output>
	);
}

export function SettingsPopover() {
	const [open, setOpen] = useState(false);
	const [token, setToken] = useState(savedAdminToken);
	const save = () => {
		saveAdminToken(token.trim());
		refetchAll();
		setOpen(false);
	};
	return (
		<div className="settings">
			<button
				type="button"
				className="btn btn-small btn-icon"
				aria-label="Connection settings"
				aria-expanded={open}
				title="Connection settings"
				onClick={() => setOpen((o) => !o)}
			>
				<Mark name="administrator" size={16} />
			</button>
			{open && (
				<div className="glass settings-pop" role="dialog" aria-label="Settings">
					<label className="field" htmlFor="admin-token">
						<span className="field-label">Gateway admin token</span>
						<input
							id="admin-token"
							className="input mono"
							type="password"
							value={token}
							placeholder="VITE_ADMIN_TOKEN if empty"
							autoComplete="off"
							onChange={(e) => setToken(e.target.value)}
							onKeyDown={(e) => e.key === "Enter" && save()}
						/>
					</label>
					<p className="faint">
						Sent as a bearer token to /admin/v1. Stored in this browser only.
					</p>
					<div className="row" style={{ gap: 8 }}>
						<button
							type="button"
							className="btn btn-primary btn-small"
							onClick={save}
						>
							Save
						</button>
						<button
							type="button"
							className="btn btn-small"
							onClick={() => {
								setToken("");
								saveAdminToken("");
								refetchAll();
							}}
						>
							Clear
						</button>
					</div>
				</div>
			)}
		</div>
	);
}
