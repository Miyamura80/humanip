// Sign up and sign in: small glass cards over better-auth email + password.
// On success the session cookie is set and the app opens on #/account.

import { type FormEvent, useState } from "react";
import { signIn, signUp } from "../api/auth";
import { refreshMe, resetMe } from "../api/resource";
import { Mark } from "../components/icons";

function AuthCard({
	mode,
	onSubmit,
}: {
	mode: "signup" | "login";
	onSubmit: (f: {
		name: string;
		email: string;
		password: string;
	}) => Promise<{ ok: true } | { ok: false; message: string }>;
}) {
	const [name, setName] = useState("");
	const [email, setEmail] = useState("");
	const [password, setPassword] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const signup = mode === "signup";

	const submit = async (e: FormEvent) => {
		e.preventDefault();
		setBusy(true);
		setError("");
		const r = await onSubmit({
			name: name.trim(),
			email: email.trim(),
			password,
		});
		if (r.ok) {
			resetMe();
			refreshMe();
			window.location.hash = "#/account";
			return;
		}
		setBusy(false);
		setError(r.message);
	};

	return (
		<div className="auth-wrap">
			<form className="glass auth-card" onSubmit={submit}>
				<span className="brand-mark auth-mark">
					<Mark name="tunnel" size={20} />
				</span>
				<h1>{signup ? "Create your account" : "Welcome back"}</h1>
				<p className="muted auth-sub">
					{signup
						? "Keys, prepaid credit and usage in one place."
						: "Sign in to your humanip account."}
				</p>
				{signup && (
					<label className="field" htmlFor="auth-name">
						<span className="field-label">Name</span>
						<input
							id="auth-name"
							className="input"
							autoComplete="name"
							required
							value={name}
							onChange={(e) => setName(e.target.value)}
						/>
					</label>
				)}
				<label className="field" htmlFor="auth-email">
					<span className="field-label">Email</span>
					<input
						id="auth-email"
						className="input"
						type="email"
						autoComplete="email"
						required
						value={email}
						onChange={(e) => setEmail(e.target.value)}
					/>
				</label>
				<label className="field" htmlFor="auth-password">
					<span className="field-label">
						Password{signup ? " (8+ characters)" : ""}
					</span>
					<input
						id="auth-password"
						className="input"
						type="password"
						autoComplete={signup ? "new-password" : "current-password"}
						minLength={signup ? 8 : undefined}
						required
						value={password}
						onChange={(e) => setPassword(e.target.value)}
					/>
				</label>
				{error && (
					<p className="form-error" role="alert">
						{error}
					</p>
				)}
				<button type="submit" className="btn btn-primary wide" disabled={busy}>
					{busy
						? signup
							? "Creating account..."
							: "Signing in..."
						: signup
							? "Create account"
							: "Sign in"}
				</button>
				<p className="faint auth-switch">
					{signup ? "Already have an account? " : "New to humanip? "}
					<a href={signup ? "#/login" : "#/signup"}>
						{signup ? "Sign in" : "Create an account"}
					</a>
				</p>
			</form>
		</div>
	);
}

export const Signup = () => <AuthCard mode="signup" onSubmit={signUp} />;

export const Login = () => <AuthCard mode="login" onSubmit={signIn} />;
