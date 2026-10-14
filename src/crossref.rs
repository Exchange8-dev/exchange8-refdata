// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Ex8:RefData cross-reference + CA calendar/entitlements (see DESIGN.md §2).
//!
//! - **Reference-data cross-reference**: symbol/ISIN/CUSIP/SEDOL
//!   resolution — many-to-one identifiers to one security, with
//!   deterministic best-match (exact, then per-scheme priority).
//! - **Corporate-action calendar + entitlement snapshot**: the CA
//!   lifecycle calendar (announced → ex → record → pay) per market, and
//!   the entitlement snapshot at record date (position × ratio).
//!
//! Design: `design/sentin8-exchange8-corporate-actions-technical-design.md`.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Identifier cross-reference
// ---------------------------------------------------------------------------

/// Identifier schemes (mirrors `exchange8_security_identifier.id_type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IdScheme {
    Symbol,
    Isin,
    Cusip,
    Sedol,
    Figi,
}

impl IdScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            IdScheme::Symbol => "SYMBOL",
            IdScheme::Isin => "ISIN",
            IdScheme::Cusip => "CUSIP",
            IdScheme::Sedol => "SEDOL",
            IdScheme::Figi => "FIGI",
        }
    }
}

/// One identifier row: (scheme, value) → security id.
#[derive(Clone, Debug, PartialEq)]
pub struct IdentifierRow {
    pub scheme: IdScheme,
    pub value: String,
    pub security_id: u32,
}

/// The cross-reference resolver.
pub struct CrossRef {
    /// (scheme, value) → security.
    by_id: HashMap<(IdScheme, String), u32>,
}

impl CrossRef {
    pub fn new(rows: &[IdentifierRow]) -> Self {
        let mut by_id = HashMap::new();
        for r in rows {
            by_id.insert((r.scheme, r.value.clone()), r.security_id);
        }
        CrossRef { by_id }
    }

    /// Exact resolution by (scheme, value).
    pub fn resolve(&self, scheme: IdScheme, value: &str) -> Option<u32> {
        self.by_id.get(&(scheme, value.to_string())).copied()
    }

    /// Resolve any scheme: exact matches first; when several schemes match,
    /// they must agree on the security (a disagreement = bad reference
    /// data → None, flagged for ops).
    pub fn resolve_any(&self, value: &str) -> Option<u32> {
        let hits: Vec<u32> = [
            IdScheme::Symbol,
            IdScheme::Isin,
            IdScheme::Cusip,
            IdScheme::Sedol,
            IdScheme::Figi,
        ]
        .iter()
        .filter_map(|s| self.resolve(*s, value))
        .collect();
        match hits.as_slice() {
            [] => None,
            [only] => Some(*only),
            many => {
                let first = many[0];
                if many.iter().all(|h| *h == first) {
                    Some(first)
                } else {
                    None // conflicting mappings — ops flag
                }
            }
        }
    }

    /// All identifiers of one security (reverse view).
    pub fn identifiers_of(&self, security_id: u32) -> Vec<(IdScheme, String)> {
        let mut out: Vec<_> = self
            .by_id
            .iter()
            .filter(|(_, sid)| **sid == security_id)
            .map(|((s, v), _)| (*s, v.clone()))
            .collect();
        out.sort_by_key(|(s, _)| s.as_str());
        out
    }
}

// ---------------------------------------------------------------------------
// CA calendar + entitlement snapshot
// ---------------------------------------------------------------------------

/// CA lifecycle milestone dates (µs epoch; None = not yet announced).
#[derive(Clone, Debug, PartialEq)]
pub struct CaDates {
    pub announced: u64,
    pub ex_date: Option<u64>,
    pub record_date: Option<u64>,
    pub pay_date: Option<u64>,
}

/// Calendar entry: one corporate action on one security.
#[derive(Clone, Debug, PartialEq)]
pub struct CalendarEntry {
    pub ca_id: u64,
    pub security_id: u32,
    pub kind: &'static str, // split | dividend | ...
    pub dates: CaDates,
}

/// The CA calendar: date-windowed queries for ops/trading feeds.
pub struct CaCalendar {
    entries: Vec<CalendarEntry>,
}

impl CaCalendar {
    pub fn new(entries: Vec<CalendarEntry>) -> Self {
        CaCalendar { entries }
    }

    /// Actions going ex on a given date (trading-halt/adjustment feed).
    pub fn ex_on(&self, date_us: u64) -> Vec<&CalendarEntry> {
        self.entries
            .iter()
            .filter(|e| e.dates.ex_date == Some(date_us))
            .collect()
    }

    /// Actions paying on a given date (entitlement settlement feed).
    pub fn pay_on(&self, date_us: u64) -> Vec<&CalendarEntry> {
        self.entries
            .iter()
            .filter(|e| e.dates.pay_date == Some(date_us))
            .collect()
    }

    /// Actions with record date on a given date (snapshot trigger).
    pub fn record_on(&self, date_us: u64) -> Vec<&CalendarEntry> {
        self.entries
            .iter()
            .filter(|e| e.dates.record_date == Some(date_us))
            .collect()
    }
}

/// A position snapshot row at record date.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionRow {
    pub account: String,
    pub security_id: u32,
    pub qty: i128,
}

/// The entitlement of one account for one action.
#[derive(Clone, Debug, PartialEq)]
pub struct Entitlement {
    pub ca_id: u64,
    pub account: String,
    /// Raw entitlement (shares for splits/stock dividends, cents for cash).
    pub amount: i128,
}

/// Snapshot entitlements at record date: positions × action ratio.
/// `per_share`: new per old (split 2:1 → 2), or cash cents per share.
pub fn snapshot_entitlements(
    positions: &[PositionRow],
    ca: &CalendarEntry,
    per_share: i128,
) -> Vec<Entitlement> {
    positions
        .iter()
        .filter(|p| p.security_id == ca.security_id)
        .map(|p| Entitlement {
            ca_id: ca.ca_id,
            account: p.account.clone(),
            amount: p.qty.saturating_mul(per_share),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<IdentifierRow> {
        vec![
            IdentifierRow {
                scheme: IdScheme::Symbol,
                value: "EX8A".into(),
                security_id: 1,
            },
            IdentifierRow {
                scheme: IdScheme::Isin,
                value: "CA000000EX8A1".into(),
                security_id: 1,
            },
            IdentifierRow {
                scheme: IdScheme::Cusip,
                value: "123456789".into(),
                security_id: 1,
            },
            IdentifierRow {
                scheme: IdScheme::Symbol,
                value: "EX8B".into(),
                security_id: 2,
            },
        ]
    }

    #[test]
    fn resolve_by_scheme_exact() {
        let x = CrossRef::new(&rows());
        assert_eq!(x.resolve(IdScheme::Symbol, "EX8A"), Some(1));
        assert_eq!(x.resolve(IdScheme::Isin, "CA000000EX8A1"), Some(1));
        assert_eq!(x.resolve(IdScheme::Symbol, "EX8B"), Some(2));
        assert_eq!(x.resolve(IdScheme::Cusip, "EX8A"), None);
        assert_eq!(x.resolve(IdScheme::Symbol, "MISSING"), None);
    }

    #[test]
    fn resolve_any_agrees_across_schemes() {
        let x = CrossRef::new(&rows());
        // Symbol resolves.
        assert_eq!(x.resolve_any("EX8A"), Some(1));
        // Unknown value.
        assert_eq!(x.resolve_any("ZZZ"), None);
    }

    #[test]
    fn conflicting_mappings_flagged() {
        // Same value mapped to two securities via different schemes →
        // resolve_any returns None (ops flag), scheme-specific still works.
        let mut rs = rows();
        rs.push(IdentifierRow {
            scheme: IdScheme::Figi,
            value: "EX8A".into(),
            security_id: 2,
        });
        let x = CrossRef::new(&rs);
        assert_eq!(x.resolve_any("EX8A"), None);
        assert_eq!(x.resolve(IdScheme::Symbol, "EX8A"), Some(1));
        assert_eq!(x.resolve(IdScheme::Figi, "EX8A"), Some(2));
    }

    #[test]
    fn identifiers_reverse_view() {
        let x = CrossRef::new(&rows());
        let ids = x.identifiers_of(1);
        assert_eq!(ids.len(), 3); // symbol + isin + cusip
        assert!(ids.contains(&(IdScheme::Symbol, "EX8A".into())));
        assert!(ids.contains(&(IdScheme::Isin, "CA000000EX8A1".into())));
    }

    #[test]
    fn calendar_windows_filter() {
        let cal = CaCalendar::new(vec![
            CalendarEntry {
                ca_id: 1,
                security_id: 3,
                kind: "split",
                dates: CaDates {
                    announced: 100,
                    ex_date: Some(200),
                    record_date: Some(210),
                    pay_date: Some(220),
                },
            },
            CalendarEntry {
                ca_id: 2,
                security_id: 4,
                kind: "dividend",
                dates: CaDates {
                    announced: 100,
                    ex_date: Some(300),
                    record_date: Some(310),
                    pay_date: Some(320),
                },
            },
        ]);
        assert_eq!(cal.ex_on(200).len(), 1);
        assert_eq!(cal.ex_on(200)[0].ca_id, 1);
        assert_eq!(cal.record_on(310).len(), 1);
        assert_eq!(cal.pay_on(320)[0].kind, "dividend");
        assert!(cal.ex_on(999).is_empty());
    }

    #[test]
    fn entitlements_snapshot_multiplies_positions() {
        let ca = CalendarEntry {
            ca_id: 7,
            security_id: 3,
            kind: "split",
            dates: CaDates {
                announced: 1,
                ex_date: Some(2),
                record_date: Some(3),
                pay_date: Some(4),
            },
        };
        let positions = vec![
            PositionRow {
                account: "A".into(),
                security_id: 3,
                qty: 100,
            },
            PositionRow {
                account: "B".into(),
                security_id: 3,
                qty: 250,
            },
            PositionRow {
                account: "C".into(),
                security_id: 9, // different security — excluded
                qty: 500,
            },
        ];
        // 2:1 split → per_share 2.
        let ents = snapshot_entitlements(&positions, &ca, 2);
        assert_eq!(ents.len(), 2);
        assert!(ents.iter().any(|e| e.account == "A" && e.amount == 200));
        assert!(ents.iter().any(|e| e.account == "B" && e.amount == 500));
        // Cash dividend 25 cents/share.
        let cash = snapshot_entitlements(&positions, &ca, 25);
        assert!(cash.iter().any(|e| e.account == "A" && e.amount == 2500));
    }
}
