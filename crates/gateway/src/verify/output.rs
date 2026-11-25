//! Render [`Report`]s as a readable table or as JSON.
//!
//! Reports carry no credentials (only the vendor endpoint), so nothing here
//! can leak a password.

use super::{Report, Status};

/// Human readable table for one report.
pub fn table(r: &Report) -> String {
    let adapter = match r.adapter {
        Some(a) => format!("{a} adapter"),
        None => "generic upstream".into(),
    };
    let via = if r.endpoint.is_empty() {
        r.protocol.to_string()
    } else {
        format!("{} {}", r.protocol, r.endpoint)
    };
    let mut out = format!("provider {} ({adapter}) via {via}\n", r.provider);
    let width = r
        .checks
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(5)
        .max(5);
    out.push_str(&format!(
        "  {:<width$}  {:<6}  {:>7}  DETAIL\n",
        "CHECK", "RESULT", "TIME"
    ));
    for c in &r.checks {
        out.push_str(&format!(
            "  {:<width$}  {:<6}  {:>7}  {}\n",
            c.name,
            c.status.as_str(),
            format!("{}ms", c.elapsed_ms),
            c.detail
        ));
    }
    let count = |s: Status| r.checks.iter().filter(|c| c.status == s).count();
    let (fails, warns) = (count(Status::Fail), count(Status::Warn));
    if fails > 0 {
        out.push_str(&format!(
            "  result: FAIL ({fails} failed, {warns} warnings)\n"
        ));
    } else {
        out.push_str(&format!("  result: PASS ({warns} warnings)\n"));
        if let Some(a) = r.adapter {
            out.push_str(&format!(
                "  the {a} adapter works with this account: the startup \
                 \"unverified adapter\" warning can be ignored for {}\n",
                r.provider
            ));
        }
    }
    out
}

/// `{"ok": bool, "reports": [...]}` for every report.
pub fn json(reports: &[Report]) -> String {
    let ok = !reports.iter().any(Report::failed);
    let items: Vec<String> = reports.iter().map(report_json).collect();
    format!("{{\"ok\":{ok},\"reports\":[{}]}}", items.join(","))
}

fn report_json(r: &Report) -> String {
    let checks: Vec<String> = r
        .checks
        .iter()
        .map(|c| {
            let samples: Vec<String> = c.samples_ms.iter().map(u64::to_string).collect();
            format!(
                "{{\"name\":{},\"status\":{},\"detail\":{},\"elapsed_ms\":{},\"samples_ms\":[{}]}}",
                quote(c.name),
                quote(&c.status.as_str().to_ascii_lowercase()),
                quote(&c.detail),
                c.elapsed_ms,
                samples.join(",")
            )
        })
        .collect();
    let adapter = r.adapter.map_or_else(|| "null".to_string(), quote);
    format!(
        "{{\"provider\":{},\"adapter\":{adapter},\"protocol\":{},\"endpoint\":{},\"ok\":{},\"checks\":[{}]}}",
        quote(&r.provider),
        quote(r.protocol),
        quote(&r.endpoint),
        !r.failed(),
        checks.join(",")
    )
}

/// JSON string literal.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::Check;

    #[test]
    fn quote_escapes() {
        assert_eq!(quote("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
    }

    #[test]
    fn table_and_json_render() {
        let r = Report {
            provider: "p".into(),
            adapter: Some("decodo"),
            protocol: "socks5",
            endpoint: "h:1".into(),
            checks: vec![Check {
                name: "reachable",
                status: Status::Pass,
                detail: "ok".into(),
                elapsed_ms: 5,
                samples_ms: vec![5],
            }],
        };
        let t = table(&r);
        assert!(t.contains("reachable") && t.contains("PASS") && t.contains("decodo adapter"));
        let j = json(&[r]);
        assert!(j.starts_with("{\"ok\":true,") && j.contains("\"samples_ms\":[5]"));
    }
}
