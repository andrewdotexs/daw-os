//! The CPU domains of the boot profile, as `dawos-tune` reports them in
//! `/run/dawos/topology.json` (ADR 0018).

use std::fs;
use std::path::Path;

/// Where `dawos-tune` writes its report.
pub const REPORT: &str = "/run/dawos/topology.json";

/// The CPU list of a domain ("live", "system", "io", "preemptive") in the
/// report at `path`; `None` when there is no report or no such domain,
/// as on a development PC.
///
/// The report is JSON with one `"name": "value"` pair per line; that
/// shape is all this reads, on purpose: a JSON library would be a
/// dependency for one line.
#[must_use]
pub fn domain(path: &Path, name: &str) -> Option<String> {
    let report = fs::read_to_string(path).ok()?;
    domain_in(&report, name)
}

fn domain_in(report: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\": \"");
    for line in report.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix(&key) {
            let value = rest.split('"').next()?;
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"{
  "version": 1,
  "profile": "285k",
  "status": "ok",
  "kernel": {
    "isolated": "4-7",
    "nohz_full": "2-7"
  },
  "domains": {
    "live": "4-7",
    "tickless": "2-7",
    "preemptive": "0-3,8-15",
    "system": "16-19",
    "io": "20-23"
  }
}"#;

    #[test]
    fn domains_are_found() {
        assert_eq!(domain_in(REPORT, "live").as_deref(), Some("4-7"));
        assert_eq!(domain_in(REPORT, "preemptive").as_deref(), Some("0-3,8-15"));
        assert_eq!(domain_in(REPORT, "io").as_deref(), Some("20-23"));
    }

    #[test]
    fn what_is_not_there_is_not_invented() {
        assert_eq!(domain_in(REPORT, "vm"), None);
        assert_eq!(domain_in("{}", "live"), None);
        assert_eq!(
            domain(Path::new("/nonexistent/topology.json"), "live"),
            None
        );
    }
}
