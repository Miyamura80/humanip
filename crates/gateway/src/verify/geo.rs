//! The two checks built on geo lookups: `country` and `not_datacenter`.

use std::net::IpAddr;

use super::{check, join_ips, Check, Status};
use crate::verify::fetch::Geo;

pub(super) fn country_check(want: Option<&str>, geos: &[(IpAddr, Result<Geo, String>)]) -> Check {
    let Some(want) = want else {
        return check(
            "country",
            Status::Skip,
            "pass --country XX to check targeting",
        );
    };
    let mut wrong = Vec::new();
    for (ip, g) in geos {
        match g {
            Err(e) => return check("country", Status::Fail, format!("{ip}: {e}")),
            Ok(g) => match g.country_code.as_deref() {
                Some(cc) if cc.eq_ignore_ascii_case(want) => {}
                other => wrong.push(format!("{ip} is {}", other.unwrap_or("unknown"))),
            },
        }
    }
    if wrong.is_empty() {
        check(
            "country",
            Status::Pass,
            format!("{} of {} exits geolocate to {want}", geos.len(), geos.len()),
        )
    } else {
        check(
            "country",
            Status::Fail,
            format!("wanted {want}: {}", wrong.join(", ")),
        )
    }
}

pub(super) fn datacenter_check(geos: &[(IpAddr, Result<Geo, String>)]) -> Check {
    let mut hosting = Vec::new();
    let mut failed = Vec::new();
    let (mut mobile, mut proxy) = (0, 0);
    for (ip, g) in geos {
        match g {
            Err(e) => failed.push(format!("{ip}: {e}")),
            Ok(g) => {
                if g.hosting == Some(true) {
                    hosting.push(*ip);
                }
                mobile += usize::from(g.mobile == Some(true));
                proxy += usize::from(g.proxy == Some(true));
            }
        }
    }
    if !hosting.is_empty() {
        return check(
            "not_datacenter",
            Status::Warn,
            format!("flagged hosting (datacenter): {}", join_ips(&hosting)),
        );
    }
    if !failed.is_empty() {
        return check(
            "not_datacenter",
            Status::Warn,
            format!("lookup failed for {}", failed.join("; ")),
        );
    }
    check(
        "not_datacenter",
        Status::Pass,
        format!(
            "{} exits, none flagged hosting ({mobile} mobile, {proxy} flagged proxy)",
            geos.len()
        ),
    )
}
