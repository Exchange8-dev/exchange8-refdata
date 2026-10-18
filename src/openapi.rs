// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! OpenAPI spec for Exchange8:Refdata.
//!
//! utoipa annotations on the service's handlers + the shared Shield
//! policy-admin surface (`sentin8::shell::policy_*` wrappers); the
//! shell-owned `/v1/health` mount is described through a spec wrapper
//! (`sentin8/design/sentin8-openapi-coverage-design.md` §2).

use utoipa::OpenApi;

pub mod w {

    /// Spec wrapper for the shell-owned `/v1/health` mount.
    #[utoipa::path(
        get,
        path = "/v1/health",
        responses(
            (status = 200, description = "Service is healthy", body = serde_json::Value),
        ),
        security(),
        tag = "refdata",
    )]
    pub async fn refdata_health() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        get,
        path = "/v1/admin/securities",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_list_securities() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/securities",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_create_security() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        get,
        path = "/v1/admin/securities/{id}",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_get_security() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        put,
        path = "/v1/admin/securities/{id}",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_update_security() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        delete,
        path = "/v1/admin/securities/{id}",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_delete_security() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/securities/{id}/update",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_update_security_post() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/securities/{id}/delete",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_delete_security_post() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        get,
        path = "/v1/admin/markets",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_list_markets() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/markets",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_create_market() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/markets/{id}/update",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_update_market_post() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        get,
        path = "/v1/admin/corporate-actions",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_list_corporate_actions() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/corporate-actions",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_announce_corporate_action() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        get,
        path = "/v1/admin/corporate-actions/calendar",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_list_ca_calendar() {}
    /// Spec wrapper for the `/v1/admin` alias route (same handler as the public path).
    #[utoipa::path(
        post,
        path = "/v1/admin/corporate-actions/{id}/cancel",
        responses(
            (status = 200, description = "OK", body = serde_json::Value),
            (status = 403, description = "Forbidden"),
        ),
        security(),
        tag = "exchange8-admin",
    )]
    pub async fn admin_cancel_corporate_action() {}
}

/// OpenAPI spec for the refdata API.
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::openapi::w::admin_list_securities,
        crate::openapi::w::admin_create_security,
        crate::openapi::w::admin_get_security,
        crate::openapi::w::admin_update_security,
        crate::openapi::w::admin_delete_security,
        crate::openapi::w::admin_update_security_post,
        crate::openapi::w::admin_delete_security_post,
        crate::openapi::w::admin_list_markets,
        crate::openapi::w::admin_create_market,
        crate::openapi::w::admin_update_market_post,
        crate::openapi::w::admin_list_corporate_actions,
        crate::openapi::w::admin_announce_corporate_action,
        crate::openapi::w::admin_list_ca_calendar,
        crate::openapi::w::admin_cancel_corporate_action,
crate::handlers::list_securities,
crate::handlers::create_security,
crate::handlers::get_security,
crate::handlers::update_security,
crate::handlers::delete_security,
crate::handlers::list_markets,
crate::handlers::create_market,
crate::handlers::update_market,
crate::handlers::list_corporate_actions,
crate::handlers::announce_corporate_action,
crate::handlers::list_ca_calendar,
crate::handlers::cancel_corporate_action,
crate::handlers::list_tick_rules,
crate::handlers::create_tick_rule,
crate::handlers::update_tick_rule,
crate::handlers::create_holiday,
crate::handlers::create_book_connection,
crate::handlers::admin_ui_page,
crate::handlers::internal_schedules_run,
crate::openapi::w::refdata_health,
        sentin8::shell::policy_list_sets,
        sentin8::shell::policy_get_set,
        sentin8::shell::policy_put_set,
        sentin8::shell::policy_delete_set,
        sentin8::shell::policy_test_set,
        sentin8::shell::policy_mode_enforce,
        sentin8::shell::policy_mode_shadow,
        sentin8::shell::policy_mode_disable,
        sentin8::shell::policy_create_set,
        sentin8::shell::policy_save_set,
        sentin8::shell::policy_flip_set,
        sentin8::shell::policy_list_decisions,
        sentin8::shell::policy_delete_relation_path,
        sentin8::shell::policy_delete_attribute_path,
        sentin8::shell::policy_list_relations,
        sentin8::shell::policy_put_relation,
        sentin8::shell::policy_delete_relation,
        sentin8::shell::policy_check_relation,
        sentin8::shell::policy_list_attributes,
        sentin8::shell::policy_put_attribute,
        sentin8::shell::policy_delete_attribute,
    ),
    components(schemas()),
    security(),
    tags(
        (name = "refdata", description = "Exchange8:Refdata"),
        (name = "exchange8-admin", description = "Exchange8 admin surface (RFW)"),
        (name = "policy", description = "Shield policy administration (shared surface)"),
    ),
)]
pub struct RefdataOpenApi;
