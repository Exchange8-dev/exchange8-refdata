// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Service self-registration data for Exchange8:REFDATA (generated; hand-tune freely).
//!
//! Output via `--print-registration` (see the platform
//! `sentin8-service-self-registration-design.md`).

use utoipa::OpenApi;

use sentin8::registration::{
    AdminScope, AdminUi, AdminUiPage, ErrorCode, M2mGrant, PgRole, PolicySetDecl, Registration,
    RelationDecl, ScheduleDecl, SqlMigration,
};

/// The Shield text of the declared scope-mirror set - public so the
/// policy wiring tests (see DESIGN.md §9) assert against the REAL seed.
///
/// Coverage contract (policy wiring tests): EVERY action the handlers
/// evaluate has a permit - the security + market/tick/holiday/book read
/// and write vocabulary, the corporate-action read/write vocabulary, and
/// the cron proxy-back's transition action (the cron workload's declared
/// `sentin8:cron:write` is the admitted scope; the platform-admin trio
/// rides every surface).
pub const SCOPE_MIRROR_POLICIES: &str = "permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"exchange8:refdata:read\") };
permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"exchange8:refdata:all\") };
permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"exchange8:order:read\") };
permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"exchange8:refdata:write\") };
permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"sentin8:sys:admin\") };
permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"sentin8:sys:org:admin\") };
permit(principal, action == Action::\"exchange8:refdata:read\", resource) when { principal.scopes.contains(\"sentin8:sys:env:admin\") };
permit(principal, action == Action::\"exchange8:refdata:write\", resource) when { principal.scopes.contains(\"exchange8:refdata:write\") };
permit(principal, action == Action::\"exchange8:refdata:write\", resource) when { principal.scopes.contains(\"sentin8:sys:admin\") };
permit(principal, action == Action::\"exchange8:refdata:write\", resource) when { principal.scopes.contains(\"sentin8:sys:org:admin\") };
permit(principal, action == Action::\"exchange8:refdata:write\", resource) when { principal.scopes.contains(\"sentin8:sys:env:admin\") };
permit(principal, action == Action::\"exchange8:corporate-action:read\", resource) when { principal.scopes.contains(\"exchange8:corporate-action:read\") };
permit(principal, action == Action::\"exchange8:corporate-action:read\", resource) when { principal.scopes.contains(\"exchange8:corporate-action:write\") };
permit(principal, action == Action::\"exchange8:corporate-action:read\", resource) when { principal.scopes.contains(\"sentin8:sys:admin\") };
permit(principal, action == Action::\"exchange8:corporate-action:read\", resource) when { principal.scopes.contains(\"sentin8:sys:org:admin\") };
permit(principal, action == Action::\"exchange8:corporate-action:read\", resource) when { principal.scopes.contains(\"sentin8:sys:env:admin\") };
permit(principal, action == Action::\"exchange8:corporate-action:write\", resource) when { principal.scopes.contains(\"exchange8:corporate-action:write\") };
permit(principal, action == Action::\"exchange8:corporate-action:write\", resource) when { principal.scopes.contains(\"sentin8:sys:admin\") };
permit(principal, action == Action::\"exchange8:corporate-action:write\", resource) when { principal.scopes.contains(\"sentin8:sys:org:admin\") };
permit(principal, action == Action::\"exchange8:corporate-action:write\", resource) when { principal.scopes.contains(\"sentin8:sys:env:admin\") };
permit(principal, action == Action::\"exchange8:refdata:corporate-action:transition\", resource) when { principal.scopes.contains(\"sentin8:cron:write\") };
permit(principal, action == Action::\"exchange8:refdata:corporate-action:transition\", resource) when { principal.scopes.contains(\"exchange8:refdata:corporate-action:transition\") };
permit(principal, action == Action::\"exchange8:refdata:corporate-action:transition\", resource) when { principal.scopes.contains(\"sentin8:sys:admin\") };
permit(principal, action == Action::\"exchange8:refdata:corporate-action:transition\", resource) when { principal.scopes.contains(\"exchange8:corporate-action:write\") };";

/// Default declarative policy set (Sentin8:Policy design §3.2/§3.4,
/// seeded by `vertical enroll` as `origin='platform'`, `mode='enforced'`):
/// action-granular scope permits over the LIVE effective scope set - the
/// exact mirror of the gates this service's handlers evaluate in step 5
/// (policy design §4.3). Enforced: step 5 is the only gate; shadow is the
/// emergency one-UPDATE rollback lever only.
const POLICIES: &[PolicySetDecl] = &[PolicySetDecl {
    policy_set_name: "scope-mirror",
    description: "Action-granular scope permits for exchange8:refdata over the LIVE effective scope set. Enforced - step 5 is the only gate.",
    // Shield text lives in SCOPE_MIRROR_POLICIES (single source).
    policies: SCOPE_MIRROR_POLICIES,
    schema: None,
    mode: "enforced",
    default_enabled: true,
}];

/// Default ReBAC relation tuples (design §3.1c) - none declared by this
/// service today; env admins author their own graph via the relations
/// admin API.
const RELATIONS: &[RelationDecl] = &[];

const PG_ROLES: &[PgRole] = &[PgRole {
    scope: "exchange8:refdata:read,exchange8:refdata:write,exchange8:corporate-action:read,exchange8:corporate-action:write",
    role: "exchange8_refdata_service",
    bypass_rls: false,
        // Grants live in the exchange8 vertical database only (principles §6.6
        // database-per-vertical, schema-per-service).
        database: "exchange8",
        // This service owns all its reads - no cross-schema grants.
        cross_schema_grants: &[],
}];

const ADMIN_SCOPES: &[AdminScope] = &[
    AdminScope::Typed {
        role: "exchange8:admin",
        scope: "exchange8:refdata:read",
    },
    AdminScope::Typed {
        role: "exchange8:admin",
        scope: "exchange8:refdata:write",
    },
    AdminScope::Typed {
        role: "exchange8:admin",
        scope: "exchange8:corporate-action:read",
    },
    AdminScope::Typed {
        role: "exchange8:admin",
        scope: "exchange8:corporate-action:write",
    },
    // The cron proxy-back transition action (DESIGN.md §10 worker): the
    // env admin role maps it so an env operator can trigger the transition
    // run manually too.
    AdminScope::Typed {
        role: "exchange8:admin",
        scope: "exchange8:refdata:corporate-action:transition",
    },
    // Policy-admin surface (policy design §3.6; principles §6.0b per-service scope).
    AdminScope::Typed {
        role: "exchange8:refdata:admin",
        scope: "exchange8:refdata:policy:read",
    },
    AdminScope::Typed {
        role: "exchange8:refdata:admin",
        scope: "exchange8:refdata:policy:write",
    },
];

const ERROR_CODES: &[ErrorCode] = &[ErrorCode {
    enum_name: "Exchange8SecurityNotFound",
    error_code: "exchange8_security_not_found",
    status_code: 404,
}];

const ADMIN_UI_PAGES: &[AdminUiPage] = &[
    AdminUiPage {
        route: "securities",
        label: "Securities",
        ui_endpoint: "/v1/admin/ui/securities",
        data_endpoint: "/v1/admin/securities",
        icon: "0xe54e",
    },
    AdminUiPage {
        route: "markets",
        label: "Markets",
        ui_endpoint: "/v1/admin/ui/markets",
        data_endpoint: "/v1/admin/markets",
        icon: "0xe8cc",
    },
    AdminUiPage {
        route: "corporate-actions",
        label: "Corporate Actions",
        ui_endpoint: "/v1/admin/ui/corporate-actions",
        data_endpoint: "/v1/admin/corporate-actions",
        icon: "0xe05e",
    },
];

/// Workload scope grants (self-registration design §3.11). One entry per
/// workload this service presents a certificate for; `vertical enroll` upserts the
/// rows so the fail-closed registry map admits the registration. Scopes use the
/// full vocabulary form:
/// - `exchange8:refdata` / `refdata` - the service's own client (both
///   spellings granted; runtime call sites use the bare short name, the
///   map looks up the exact workload): bus produce/consume + the
///   shared certificate identity.
/// - `exchange8:refdata-policy` / `refdata-policy` - Shield hot-reload
///   consumer + stat emitter pair.
/// - `refdata-audit` - shell audit-journal pusher workload (see DESIGN.md §5.5).
/// - `exchange8-refdata-tls` - database-free HostContextResolver resolve workload
///   (TLS-RLS-4: `GET /v1/resolve/hostname`, scope
///   `sentin8:sys:instances:read`).
const M2M_GRANTS: &[M2mGrant] = &[
    M2mGrant {
        workload: "exchange8:refdata",
        scopes: &[
            "exchange8:refdata:read",
            "exchange8:refdata:write",
            "exchange8:corporate-action:read",
            "exchange8:corporate-action:write",
            "sentin8:bus:produce",
            "sentin8:bus:consume",
            "sentin8:stat:measure",
            "sentin8:cron:write",
        ],
    },
    M2mGrant {
        workload: "refdata",
        scopes: &[
            "exchange8:refdata:read",
            "exchange8:refdata:write",
            "exchange8:corporate-action:read",
            "exchange8:corporate-action:write",
            "sentin8:bus:produce",
            "sentin8:bus:consume",
            "sentin8:stat:measure",
            "sentin8:cron:write",
        ],
    },
    // Corporate-action transition schedule self-registration (DESIGN.md
    // §10 worker): the boot registers the recurring `refdata-ca-transitions`
    // schedule with Sentin8:Cron; Cron's ticker proxy-back authenticates
    // the dedicated `refdata-cron` workload (secrets-server precedent).
    M2mGrant {
        workload: "refdata-cron",
        scopes: &["sentin8:cron:write"],
    },
    M2mGrant {
        workload: "exchange8:refdata-policy",
        scopes: &["sentin8:bus:consume", "sentin8:stat:measure"],
    },
    M2mGrant {
        workload: "refdata-policy",
        scopes: &["sentin8:stat:measure"],
    },
    M2mGrant {
        workload: "refdata-audit",
        scopes: &["sentin8:sys:audit:write"],
    },
    M2mGrant {
        workload: "exchange8-refdata-tls",
        scopes: &["sentin8:sys:instances:read"],
    },
];

const SQL_1_INIT: &str = include_str!("../sql/init.sql");

fn empty_openapi() -> serde_json::Value {
    serde_json::to_value(crate::openapi::RefdataOpenApi::openapi()).unwrap_or(serde_json::Value::Null)
}

pub fn print(section: Option<String>) -> anyhow::Result<()> {
    let reg = Registration {
        service_type: "exchange8:refdata",
        subdomain: "exchange8-refdata",
        health_path: "/v1/health",
        admin_scopes: ADMIN_SCOPES,
        error_codes: ERROR_CODES,
        consent_providers: &[],
        consent_flows: &[],
        admin_ui: Some(AdminUi {
            label: "Reference Data",
            icon: "0xe8b0",
            service_flag: "exchange8-refdata",
            pages: ADMIN_UI_PAGES,
        }),
        sql_migrations: vec![SqlMigration {
            version: 1,
            name: "init",
            sql: SQL_1_INIT,
        }],
        pg_roles: PG_ROLES,
        audit_findings: None,
        service_actions: None,
        m2m_grants: Some(M2M_GRANTS),
        openapi_url: "/openapi.json",
        openapi: empty_openapi,
        policies: POLICIES,
        relations: RELATIONS,
        event_triggers: &[],
        // Corporate-action transition sweep (SDD §2.4): every 5 minutes -
        // the cron clock drives due-stage advancement; the proxy-back
        // handler runs the stage-predicated transition worker. Constants in
        // `worker.rs` keep the contract honest.
        schedules: &[ScheduleDecl {
            schedule_type: "refdata-ca-transitions",
            cron_expression: "*/5 * * * *",
            timezone: "UTC",
            target_config: r#"{"internalPath": "/v1/internal/schedules/run", "scheduleType": "refdata-ca-transitions"}"#,
            max_retries: 3,
        }],
    };
    sentin8::registration::print_registration(section, &reg, empty_openapi)
}
