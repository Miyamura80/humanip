// End-to-end product test against the local stack (scripts/dev-stack.sh start):
// sign up -> account ($0) -> create key -> top up via fake Stripe -> balance
// credited -> traffic through the gateway with the new key -> usage billed.
//
//   node scripts/e2e-product.cjs            (screenshots in .stack/e2e/)
//
// Exits non-zero on the first failed step.

const { chromium } = require("playwright");
const { execFileSync, spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const CONSOLE = process.env.CONSOLE_URL || "http://localhost:1420";
const PROXY = process.env.GATEWAY_HTTP || "127.0.0.1:8080";
const OUT = path.join(__dirname, "..", ".stack", "e2e");
fs.mkdirSync(OUT, { recursive: true });

const step = (n, msg) => console.log(`[${n}] ${msg}`);
const fail = (msg) => {
	console.error(`FAIL: ${msg}`);
	process.exit(1);
};

/** Parse "$12.34" (first dollar amount in a string) to a number. */
const dollars = (s) => {
	const m = /\$([\d,]+\.\d{2})/.exec(s || "");
	return m ? Number(m[1].replace(/,/g, "")) : Number.NaN;
};

async function balance(page) {
	const t = await page.locator("header.topnav").innerText();
	return dollars(t);
}

(async () => {
	// A tiny origin for proxied traffic.
	const origin = spawn(
		"python3",
		["-m", "http.server", "18104", "--bind", "127.0.0.1"],
		{
			stdio: "ignore",
			cwd: OUT,
		},
	);
	const proxy = process.env.HTTPS_PROXY
		? { server: process.env.HTTPS_PROXY, bypass: "localhost,127.0.0.1" }
		: undefined;
	const browser = await chromium.launch({ proxy });
	const page = await browser.newPage({
		viewport: { width: 1440, height: 900 },
	});
	const shot = (name) =>
		page.screenshot({ path: path.join(OUT, `${name}.png`), fullPage: true });
	const email = `e2e+${Date.now()}@humanip.test`;
	try {
		step(1, "landing page");
		await page.goto(`${CONSOLE}/#/`);
		await page.waitForLoadState("networkidle");
		await shot("1-landing");

		step(2, `sign up as ${email}`);
		await page.goto(`${CONSOLE}/#/signup`);
		await page.fill("#auth-name", "E2E Dev");
		await page.fill("#auth-email", email);
		await page.fill("#auth-password", "correct-horse-battery-9");
		await page.click("button[type=submit]");
		await page.waitForURL(/#\/account/, { timeout: 15000 });
		await page.getByText("Prepaid balance").waitFor({ timeout: 15000 });
		await page.waitForTimeout(1500);
		const b0 = await balance(page);
		await shot("2-account-new");
		if (b0 !== 0) fail(`new account balance should be $0.00, got ${b0}`);

		step(3, "create an API key");
		await page.getByRole("button", { name: /new key/i }).click();
		await page.fill('input[placeholder="e.g. browser-agent"]', "e2e-laptop");
		await page
			.getByRole("dialog")
			.getByRole("button", { name: /create/i })
			.click();
		const secretEl = page.getByText(/hip_live_[A-Za-z0-9]{20,}/).first();
		await secretEl.waitFor({ timeout: 15000 });
		const secret = /hip_live_[A-Za-z0-9]+/.exec(await secretEl.innerText())[0];
		await shot("3-new-key");
		await page.getByRole("dialog").getByRole("button").last().click();

		step(4, "top up $25 through fake Stripe");
		await page
			.getByRole("button", { name: /^top up$/i })
			.first()
			.click();
		await page
			.getByRole("dialog")
			.getByRole("button", { name: "$25", exact: true })
			.click();
		await page
			.getByRole("dialog")
			.getByRole("button", { name: /^pay /i })
			.click();
		await page.waitForURL(/12111\/pay\//, { timeout: 15000 });
		await shot("4-fake-stripe");
		await page.getByRole("button", { name: /^pay/i }).click();
		await page.waitForURL(new RegExp(CONSOLE.replace(/[.:/]/g, "\\$&")), {
			timeout: 15000,
		});
		let b1 = Number.NaN;
		for (let i = 0; i < 40 && !(b1 >= 25); i++) {
			await page.waitForTimeout(500);
			b1 = await balance(page);
		}
		await shot("5-after-topup");
		if (b1 !== 25) fail(`balance after $25 top-up should be $25.00, got ${b1}`);

		step(5, "send traffic through the gateway with the new key");
		const code = execFileSync(
			"curl",
			[
				"--noproxy",
				"",
				"-s",
				"-o",
				"/dev/null",
				"-w",
				"%{http_code}",
				"-x",
				`http://user-session_e2e:${secret}@${PROXY}`,
				"http://127.0.0.1:18104/",
			],
			{ encoding: "utf8" },
		);
		if (code !== "200") fail(`proxied request should be 200, got ${code}`);
		const bad = execFileSync(
			"curl",
			[
				"--noproxy",
				"",
				"-s",
				"-o",
				"/dev/null",
				"-w",
				"%{http_code}",
				"-x",
				`http://user:hip_live_wrong@${PROXY}`,
				"http://127.0.0.1:18104/",
			],
			{ encoding: "utf8" },
		);
		if (bad !== "407") fail(`wrong key should be 407, got ${bad}`);

		step(6, "usage billed and visible on the account page");
		let b2 = b1;
		for (let i = 0; i < 20 && b2 >= b1; i++) {
			await page.waitForTimeout(1000);
			await page.reload();
			await page.getByText("Prepaid balance").waitFor();
			await page.waitForTimeout(800);
			b2 = await balance(page);
		}
		await shot("6-after-traffic");
		const body = await page.locator("main").innerText();
		if (!/127\.0\.0\.1/.test(body))
			fail("usage should list the 127.0.0.1 domain");
		// $1/GB dev provider: a few KB costs well under a cent, so the balance
		// may still render as $25.00. Check the ledger-backed API directly too.
		const me = await page.evaluate(async () =>
			(await fetch("/app/api/me")).json(),
		);
		if (!(me.account.balance_usd < 25))
			fail(`balance should drop below $25, got ${me.account.balance_usd}`);
		step(
			"ok",
			`signup -> key -> top-up -> traffic -> billed (balance $${me.account.balance_usd})`,
		);
	} finally {
		await browser.close();
		origin.kill();
	}
})().catch((e) => fail(e.stack || String(e)));
