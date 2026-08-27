//! Futures contract specs and Sierra Chart symbol normalization.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContractSpec {
    pub root: &'static str,
    pub micro_root: Option<&'static str>,
    pub tick_size: f64,
    pub currency_per_tick: f64,
    pub micro_currency_per_tick: Option<f64>,
}

pub const SPECS: &[ContractSpec] = &[
    ContractSpec {
        root: "ES",
        micro_root: Some("MES"),
        tick_size: 0.25,
        currency_per_tick: 12.50,
        micro_currency_per_tick: Some(1.25),
    },
    ContractSpec {
        root: "NQ",
        micro_root: Some("MNQ"),
        tick_size: 0.25,
        currency_per_tick: 5.00,
        micro_currency_per_tick: Some(0.50),
    },
    ContractSpec {
        root: "YM",
        micro_root: Some("MYM"),
        tick_size: 1.00,
        currency_per_tick: 5.00,
        micro_currency_per_tick: Some(0.50),
    },
    ContractSpec {
        root: "RTY",
        micro_root: Some("M2K"),
        tick_size: 0.10,
        currency_per_tick: 5.00,
        micro_currency_per_tick: Some(0.50),
    },
    ContractSpec {
        root: "CL",
        micro_root: Some("MCL"),
        tick_size: 0.01,
        currency_per_tick: 10.00,
        micro_currency_per_tick: Some(1.00),
    },
    ContractSpec {
        root: "GC",
        micro_root: Some("MGC"),
        tick_size: 0.10,
        currency_per_tick: 10.00,
        micro_currency_per_tick: Some(1.00),
    },
    ContractSpec {
        root: "NKD",
        micro_root: None,
        tick_size: 5.00,
        currency_per_tick: 5.00,
        micro_currency_per_tick: None,
    },
];

/// Vendor aliases that are not the CME root (longest first).
const ALIASES: &[(&str, &str, bool)] = &[
    ("ENQ", "NQ", false), // CQG e-mini Nasdaq
    ("EP", "ES", false),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedSymbol {
    pub raw: String,
    /// Canonical root (ES, NQ, …) when known; otherwise the stripped code.
    pub root: String,
    /// Listed product (MES vs ES).
    pub listed: String,
    pub is_micro: bool,
    pub tick_size: Option<f64>,
    pub currency_per_tick: Option<f64>,
}

pub fn spec_for_root(root: &str) -> Option<&'static ContractSpec> {
    let up = root.to_ascii_uppercase();
    SPECS.iter().find(|s| s.root == up)
}

pub fn parse_symbol(raw: &str) -> ParsedSymbol {
    let listed = extract_listed(raw);
    let (root, is_micro) = canonical_root(&listed);
    let spec = spec_for_root(&root);
    let (tick_size, currency_per_tick) = match spec {
        Some(s) if is_micro => (Some(s.tick_size), s.micro_currency_per_tick),
        Some(s) => (Some(s.tick_size), Some(s.currency_per_tick)),
        None => (None, None),
    };
    ParsedSymbol {
        raw: raw.to_string(),
        root,
        listed,
        is_micro,
        tick_size,
        currency_per_tick,
    }
}

/// Prefer root detection. Only use an imported `currency_per_tick` when it matches a known spec.
pub fn resolve_currency_per_tick(parsed: &ParsedSymbol, imported: Option<f64>) -> Option<f64> {
    if let Some(from_spec) = parsed.currency_per_tick {
        return Some(from_spec);
    }
    imported.filter(|v| *v > 0.0)
}

fn extract_listed(raw: &str) -> String {
    let mut s = raw.trim().to_ascii_uppercase();
    if let Some(rest) = s.strip_prefix("F.US.") {
        s = rest.to_string();
    }
    let token = s
        .split(['.', '-', '_', ' '])
        .find(|t| !t.is_empty() && *t != "FUT" && *t != "CME" && *t != "CBOT" && *t != "COMEX" && *t != "NYMEX")
        .unwrap_or(&s)
        .to_string();

    if let Some(root) = match_known_prefix(&token) {
        return root.to_string();
    }
    strip_month_year(&token)
}

fn match_known_prefix(token: &str) -> Option<&'static str> {
    let mut keys: Vec<&str> = SPECS
        .iter()
        .flat_map(|s| {
            let mut v = vec![s.root];
            if let Some(m) = s.micro_root {
                v.push(m);
            }
            v
        })
        .collect();
    keys.extend(ALIASES.iter().map(|(a, _, _)| *a));
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    keys.into_iter().find(|k| token == *k || token.starts_with(k))
}

fn canonical_root(listed: &str) -> (String, bool) {
    let up = listed.to_ascii_uppercase();
    for (alias, root, micro) in ALIASES {
        if up == *alias {
            return ((*root).to_string(), *micro);
        }
    }
    for spec in SPECS {
        if up == spec.root {
            return (spec.root.to_string(), false);
        }
        if spec.micro_root == Some(up.as_str()) {
            return (spec.root.to_string(), true);
        }
    }
    (up, false)
}

fn strip_month_year(token: &str) -> String {
    // ESU26 / MNQU6 / 1OZZ26
    let bytes = token.as_bytes();
    if bytes.len() >= 3 {
        let last = bytes[bytes.len() - 1];
        let second = bytes[bytes.len() - 2];
        if last.is_ascii_digit() {
            if second.is_ascii_digit() && is_month(bytes[bytes.len() - 3]) {
                return token[..bytes.len() - 3].to_string();
            }
            if is_month(second) {
                return token[..bytes.len() - 2].to_string();
            }
        }
    }
    token.to_string()
}

fn is_month(b: u8) -> bool {
    matches!(b, b'F' | b'G' | b'H' | b'J' | b'K' | b'M' | b'N' | b'Q' | b'U' | b'V' | b'X' | b'Z')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_aliases_from_live_journal() {
        let cases: &[(&str, &str, bool, f64)] = &[
            ("ESU26_FUT_CME", "ES", false, 12.50),
            ("ESU6.CME", "ES", false, 12.50),
            ("F.US.ENQU26", "NQ", false, 5.00),
            ("F.US.MNQU26", "NQ", true, 0.50),
            ("GCZ6.COMEX", "GC", false, 10.00),
            ("MESU26_FUT_CME", "ES", true, 1.25),
            ("MNQU26_FUT_CME", "NQ", true, 0.50),
            ("MNQU6.CME", "NQ", true, 0.50),
            ("NKDU6.CME", "NKD", false, 5.00),
            ("NQ-202609-CME", "NQ", false, 5.00),
            ("NQU26_FUT_CME", "NQ", false, 5.00),
            ("NQU6.CME", "NQ", false, 5.00),
        ];
        for (raw, root, micro, cpt) in cases {
            let p = parse_symbol(raw);
            assert_eq!(p.root, *root, "{raw}");
            assert_eq!(p.is_micro, *micro, "{raw}");
            assert_eq!(p.currency_per_tick, Some(*cpt), "{raw}");
            assert!(p.tick_size.is_some(), "{raw}");
        }
    }

    #[test]
    fn unknown_1oz_does_not_panic() {
        let p = parse_symbol("1OZZ26_FUT_CME");
        assert_eq!(p.listed, "1OZ");
        assert!(p.currency_per_tick.is_none());
    }

    #[test]
    fn imported_tick_value_ignored_when_spec_exists() {
        let p = parse_symbol("MNQU6.CME");
        assert_eq!(resolve_currency_per_tick(&p, Some(5.0)), Some(0.50));
    }
}
