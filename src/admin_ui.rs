// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Ex8:Dash RFW admin page definitions.
//!
//! Each service exposes `GET /v1/admin/ui/{page}` returning an RFW
//! definition JSON (Sentin8:Dash renders it — services never ship UI code).
//! Data endpoints: `POST /v1/admin/{resource}` (RFW renderer only supports
//! POST, so update/delete are POST-alias routes).
//!
//! This module holds the per-service page definitions as pure data — the
//! shell mounts them at the admin router. Icons are Material Icons code
//! points (hex), not names (architecture-principles §4.15).
//!
//! Design: `design/sentin8-exchange8-apps-technical-design.md` §1,
//! `design/sentin8-exchange8-surveillance-technical-design.md`;
//! service docs: services/refdata/DESIGN.md.

use serde_json::{json, Value};

/// One RFW page definition (the shape Dash renders).
#[derive(Clone, Debug, PartialEq)]
pub struct RfwPage {
    /// Route segment: served at /v1/admin/ui/{slug}.
    pub slug: &'static str,
    /// Page title.
    pub title: &'static str,
    /// Material Icons code point (hex string, not a name).
    pub icon_hex: &'static str,
    /// Required admin scope.
    pub scope: &'static str,
    /// Sections (RFW blocks: tables, forms, actions).
    pub sections: Vec<RfwSection>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RfwSection {
    /// Read-only table bound to a resource.
    Table {
        heading: &'static str,
        resource: &'static str,
        columns: Vec<(&'static str, &'static str)>, // (key, label)
    },
    /// Action button bound to a POST route.
    Action {
        heading: &'static str,
        resource: &'static str,
        label: &'static str,
        fields: Vec<(&'static str, &'static str)>, // (key, label) inputs
    },
}

impl RfwPage {
    /// The RFW definition JSON served at GET /v1/admin/ui/{slug}.
    pub fn to_rfw(&self) -> Value {
        let sections: Vec<Value> = self
            .sections
            .iter()
            .map(|s| match s {
                RfwSection::Table {
                    heading,
                    resource,
                    columns,
                } => json!({
                    "type": "table",
                    "heading": heading,
                    "resource": resource,
                    "columns": columns.iter()
                        .map(|(k, l)| json!({"key": k, "label": l}))
                        .collect::<Vec<_>>(),
                }),
                RfwSection::Action {
                    heading,
                    resource,
                    label,
                    fields,
                } => json!({
                    "type": "action",
                    "heading": heading,
                    "resource": resource,
                    "label": label,
                    "fields": fields.iter()
                        .map(|(k, l)| json!({"key": k, "label": l}))
                        .collect::<Vec<_>>(),
                }),
            })
            .collect();
        json!({
            "slug": self.slug,
            "title": self.title,
            "icon": self.icon_hex,
            "scope": self.scope,
            "sections": sections,
        })
    }
}

/// RefData admin pages (DESIGN.md §4 admin/RFW surface): the three pages
/// the registration declares (`securities` / `markets` / `corporate-
/// actions`). The page `scope` field is the Shield action the UI-route
/// gate evaluates (the mirror permits the refdata/CA vocabularies —
/// registration.rs `SCOPE_MIRROR_POLICIES`).
pub fn refdata_pages() -> Vec<RfwPage> {
    vec![
        RfwPage {
            slug: "securities",
            title: "Securities",
            icon_hex: "e30b", // icons.description
            scope: "exchange8:refdata:read",
            sections: vec![
                RfwSection::Table {
                    heading: "Security master",
                    resource: "securities",
                    columns: vec![
                        ("symbol", "Symbol"),
                        ("security_type", "Type"),
                        ("trading_state", "State"),
                        ("shortable", "Shortable"),
                    ],
                },
                RfwSection::Action {
                    heading: "New security",
                    resource: "securities",
                    label: "Create",
                    fields: vec![
                        ("symbol", "Symbol"),
                        ("security_type", "Type"),
                        ("currency", "Currency"),
                    ],
                },
            ],
        },
        RfwPage {
            slug: "markets",
            title: "Markets",
            icon_hex: "e8cc", // icons.schedule (market sessions)
            scope: "exchange8:refdata:read",
            sections: vec![
                RfwSection::Table {
                    heading: "Markets",
                    resource: "markets",
                    columns: vec![
                        ("code", "Code"),
                        ("name", "Name"),
                        ("market_type", "Type"),
                        ("market_mode", "Mode"),
                        ("lot_size", "Lot size"),
                    ],
                },
                RfwSection::Action {
                    heading: "New market",
                    resource: "markets",
                    label: "Create",
                    fields: vec![("code", "Code"), ("name", "Name")],
                },
                RfwSection::Action {
                    heading: "Update market",
                    resource: "markets/update",
                    label: "Update",
                    fields: vec![
                        ("id", "Market ID"),
                        ("name", "Name"),
                        ("lot_size", "Lot size"),
                    ],
                },
            ],
        },
        RfwPage {
            slug: "corporate-actions",
            title: "Corporate Actions",
            icon_hex: "e05e", // icons.event_repeat (lifecycle)
            scope: "exchange8:corporate-action:read",
            sections: vec![
                RfwSection::Table {
                    heading: "Announced actions",
                    resource: "corporate-actions",
                    columns: vec![
                        ("security_id", "Security"),
                        ("action_type", "Type"),
                        ("stage", "Stage"),
                        ("ex_date", "Ex date"),
                        ("pay_date", "Pay date"),
                    ],
                },
                RfwSection::Action {
                    heading: "Announce action",
                    resource: "corporate-actions",
                    label: "Announce",
                    fields: vec![
                        ("security_id", "Security ID"),
                        ("action_type", "Type"),
                        ("ex_date", "Ex date (YYYY-MM-DD)"),
                    ],
                },
                RfwSection::Action {
                    heading: "Cancel action",
                    resource: "corporate-actions/cancel",
                    label: "Cancel",
                    fields: vec![("id", "Action ID")],
                },
            ],
        },
    ]
}

/// Gateway admin pages (connections + sessions).
pub fn gateway_pages() -> Vec<RfwPage> {
    vec![RfwPage {
        slug: "connections",
        title: "Connections",
        icon_hex: "e8b8", // icons.hub
        scope: "exchange8:gateway:connections:admin",
        sections: vec![
            RfwSection::Table {
                heading: "FIX/OUCH sessions",
                resource: "connections",
                columns: vec![
                    ("client_comp_id", "CompID"),
                    ("protocol", "Protocol"),
                    ("status", "Status"),
                    ("certification_status", "Certified"),
                ],
            },
            RfwSection::Action {
                heading: "Suspend session",
                resource: "connections/suspend",
                label: "Suspend",
                fields: vec![("client_comp_id", "CompID")],
            },
        ],
    }]
}

/// ME admin pages (market control wall).
pub fn me_pages() -> Vec<RfwPage> {
    vec![RfwPage {
        slug: "market-control",
        title: "Market Control",
        icon_hex: "e8ac", // icons.tune
        scope: "exchange8:me:market:admin",
        sections: vec![
            RfwSection::Table {
                heading: "Instrument states",
                resource: "instruments",
                columns: vec![
                    ("instrument_id", "ID"),
                    ("symbol", "Symbol"),
                    ("status", "Status"),
                ],
            },
            RfwSection::Action {
                heading: "Halt / resume",
                resource: "instruments/state",
                label: "Apply",
                fields: vec![
                    ("instrument_id", "Instrument ID"),
                    ("status", "Status (O/H/Q)"),
                ],
            },
        ],
    }]
}

/// Clearing admin pages (trades + bust).
pub fn clearing_pages() -> Vec<RfwPage> {
    vec![RfwPage {
        slug: "trades",
        title: "Trades",
        icon_hex: "e872", // icons.receipt
        scope: "exchange8:clearing:trades:admin",
        sections: vec![
            RfwSection::Table {
                heading: "Executed trades",
                resource: "trades",
                columns: vec![
                    ("trade_id", "Trade"),
                    ("instrument_id", "Instrument"),
                    ("qty", "Qty"),
                    ("price", "Price"),
                ],
            },
            RfwSection::Action {
                heading: "Bust trade (step-up)",
                resource: "trades/bust",
                label: "Bust",
                fields: vec![("trade_id", "Trade ID")],
            },
        ],
    }]
}

/// Surveillance case-management surface.
pub fn surveillance_pages() -> Vec<RfwPage> {
    vec![RfwPage {
        slug: "cases",
        title: "Surveillance Cases",
        icon_hex: "e8f2", // icons.gavel (case review)
        scope: "exchange8:surveillance:cases:admin",
        sections: vec![
            RfwSection::Table {
                heading: "Open cases",
                resource: "cases",
                columns: vec![
                    ("detector", "Detector"),
                    ("severity", "Severity"),
                    ("account", "Account"),
                    ("ts_us", "Raised"),
                ],
            },
            RfwSection::Action {
                heading: "Close case",
                resource: "cases/close",
                label: "Close",
                fields: vec![("case_id", "Case ID")],
            },
        ],
    }]
}

/// VFeeds admin page (feed status + retransmission config).
pub fn vfeeds_pages() -> Vec<RfwPage> {
    vec![RfwPage {
        slug: "feeds",
        title: "Venue Feeds",
        icon_hex: "e629", // icons.settings_input_antenna
        scope: "exchange8:vfeeds:feeds:admin",
        sections: vec![RfwSection::Table {
            heading: "External feed adapters",
            resource: "feeds",
            columns: vec![
                ("adapter", "Adapter"),
                ("transport", "Transport"),
                ("status", "Status"),
                ("last_seq", "Last sequence"),
            ],
        }],
    }]
}

/// All pages (registration payload: vertical-registry admin_ui entries).
pub fn all_pages() -> Vec<RfwPage> {
    let mut pages = Vec::new();
    pages.extend(refdata_pages());
    pages.extend(gateway_pages());
    pages.extend(me_pages());
    pages.extend(clearing_pages());
    pages.extend(surveillance_pages());
    pages.extend(vfeeds_pages());
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_render_rfw_json() {
        for page in all_pages() {
            let v = page.to_rfw();
            assert_eq!(v["slug"], page.slug);
            assert!(
                v["icon"]
                    .as_str()
                    .unwrap()
                    .chars()
                    .all(|c| c.is_ascii_hexdigit()),
                "icon must be a Material code point (hex)"
            );
            assert!(v["scope"].as_str().unwrap().starts_with("exchange8:"));
            assert!(!v["sections"].as_array().unwrap().is_empty());
            for s in v["sections"].as_array().unwrap() {
                assert!(s["resource"].is_string());
            }
        }
    }

    #[test]
    fn every_service_has_pages() {
        let all = all_pages();
        assert!(all.len() >= 7); // refdata×3 + gateway + me + clearing + surveillance + vfeeds
        let slugs: Vec<&str> = all.iter().map(|p| p.slug).collect();
        assert!(slugs.contains(&"securities"), "refdata security master");
        assert!(slugs.contains(&"markets"), "refdata market surface");
        assert!(
            slugs.contains(&"corporate-actions"),
            "refdata corporate-action lifecycle"
        );
        assert!(slugs.contains(&"cases"), "SV1.4 case surface");
        assert!(slugs.contains(&"market-control"));
        assert!(slugs.contains(&"trades"));
    }

    #[test]
    fn refdata_pages_match_the_registration_routes() {
        // The registration (registration.rs ADMIN_UI_PAGES) declares the
        // routes; the RFW definitions carry the same slugs (DESIGN.md §4).
        let slugs: Vec<&str> = refdata_pages().iter().map(|p| p.slug).collect();
        assert_eq!(slugs, vec!["securities", "markets", "corporate-actions"]);
        // Every page's gate scope is one the mirror admits (registration.rs
        // SCOPE_MIRROR_POLICIES vocabulary).
        for p in refdata_pages() {
            assert!(
                p.scope == "exchange8:refdata:read" || p.scope == "exchange8:corporate-action:read",
                "{}: page scope {} is not a declared mirror action",
                p.slug,
                p.scope
            );
        }
    }

    #[test]
    fn rfw_uses_post_friendly_resources() {
        // RFW renderer only supports POST: action resources are POST-alias
        // routes (e.g. trades/bust), never raw PUT/DELETE verbs.
        for page in all_pages() {
            for s in &page.sections {
                if let RfwSection::Action { resource, .. } = s {
                    assert!(!resource.contains("PUT"));
                    assert!(!resource.contains("DELETE"));
                }
            }
        }
    }
}
