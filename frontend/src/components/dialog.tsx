// Modal glass dialog on the native <dialog> element (focus trap, Escape and
// the top layer come for free), plus the one-time secret reveal.

import { type ReactNode, useEffect, useRef } from "react";
import { Mark } from "./icons";
import { CopyField } from "./ui";

export function Dialog({
	title,
	onClose,
	children,
	wide,
}: {
	title: string;
	onClose: () => void;
	children: ReactNode;
	wide?: boolean;
}) {
	const ref = useRef<HTMLDialogElement>(null);
	useEffect(() => {
		const d = ref.current;
		if (d && !d.open) d.showModal();
		return () => d?.close();
	}, []);
	return (
		<dialog
			ref={ref}
			className={`glass modal${wide ? " modal-wide" : ""}`}
			aria-label={title}
			onCancel={(e) => {
				e.preventDefault();
				onClose();
			}}
		>
			<div className="row spread modal-head">
				<h2>{title}</h2>
				<button
					type="button"
					className="btn btn-small btn-icon"
					aria-label="Close"
					onClick={onClose}
				>
					<span aria-hidden="true">×</span>
				</button>
			</div>
			{children}
		</dialog>
	);
}

/** A key secret, shown exactly once. */
export function SecretReveal({
	secret,
	onDone,
}: {
	secret: string;
	onDone: () => void;
}) {
	return (
		<div className="stack" style={{ gap: 14 }}>
			<CopyField value={secret} />
			<div className="note note-warn">
				<Mark name="shield-alert" size={18} />
				<span>Copy it now. You won't see this secret again.</span>
			</div>
			<button type="button" className="btn btn-primary" onClick={onDone}>
				I've saved it
			</button>
		</div>
	);
}
