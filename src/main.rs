// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Exchange8:REFDATA service binary - instrument reference data + corporate actions.
//!
//! Detailed design: services/refdata/DESIGN.md. Master design:
//! `design/sentin8-exchange8-refdata-technical-design.md`. Core logic in
//! the sibling modules; this shell wires the tokio HTTP/TLS surface via
//! the platform `sentin8-shell` crate (registration, DB pool, health).
//! The corporate-action transition worker is Sentin8:Cron-driven
//! (DESIGN.md §10 worker): the boot registers the recurring schedule and
//! the router mounts the `POST /v1/internal/schedules/run` proxy-back -
//! no in-process scheduler (design §1 delegation table).

#![allow(dead_code)]

// (see DESIGN.md §5). Workers (outbox relay, audit pusher, the Cron
// schedule registration) are in-process tokio tasks, so the binary-level
mod admin_ui;
mod crossref;
mod events;
mod handlers;
mod openapi;
mod refdata;
mod registration;
mod worker;

use std::sync::Arc;

fn extend_router(
    app: axum::Router<std::sync::Arc<sentin8::shell::AppState>>,
    _state: Arc<sentin8::shell::AppState>,
) -> axum::Router<Arc<sentin8::shell::AppState>> {
    // ── Sentin8:Policy admin API (shared implementation; the shell
    // AppState implements HasPolicyAdmin).
    sentin8::shell::mount_policy_admin(
        // Reference-data API (RLS-scoped via the platform authorization
        // pipeline) + the admin RFW surface (page definitions + data
        // endpoints; POST-alias routes - renderer is POST-only) + the
        // Sentin8:Cron proxy-back target (DESIGN.md §10 worker).
        app.route(
            "/v1/securities",
            axum::routing::get(handlers::list_securities).post(handlers::create_security),
        )
        .route(
            "/v1/securities/{id}",
            axum::routing::get(handlers::get_security)
                .put(handlers::update_security)
                .delete(handlers::delete_security),
        )
        .route(
            "/v1/markets",
            axum::routing::get(handlers::list_markets).post(handlers::create_market),
        )
        .route(
            "/v1/markets/{id}",
            axum::routing::put(handlers::update_market),
        )
        .route(
            "/v1/corporate-actions",
            axum::routing::get(handlers::list_corporate_actions)
                .post(handlers::announce_corporate_action),
        )
        .route(
            "/v1/corporate-actions/calendar",
            axum::routing::get(handlers::list_ca_calendar),
        )
        .route(
            "/v1/corporate-actions/{id}/cancel",
            axum::routing::post(handlers::cancel_corporate_action),
        )
        .route(
            "/v1/tick-rules",
            axum::routing::get(handlers::list_tick_rules).post(handlers::create_tick_rule),
        )
        .route(
            "/v1/tick-rules/{id}",
            axum::routing::put(handlers::update_tick_rule),
        )
        .route(
            "/v1/holidays",
            axum::routing::post(handlers::create_holiday),
        )
        .route(
            "/v1/books/connections",
            axum::routing::post(handlers::create_book_connection),
        )
        // ── Admin RFW page definitions (Dash renders them; the service
        // mounts them - the shell does not auto-serve declared pages).
        .route(
            "/v1/admin/ui/{page}",
            axum::routing::get(handlers::admin_ui_page),
        )
        // ── Admin data endpoints (RFW data_endpoint fetches + POST-alias
        // mutations - the renderer only supports POST).
        .route(
            "/v1/admin/securities",
            axum::routing::get(handlers::list_securities).post(handlers::create_security),
        )
        .route(
            "/v1/admin/securities/{id}",
            axum::routing::get(handlers::get_security)
                .put(handlers::update_security)
                .delete(handlers::delete_security),
        )
        .route(
            "/v1/admin/securities/{id}/update",
            axum::routing::post(handlers::update_security),
        )
        .route(
            "/v1/admin/securities/{id}/delete",
            axum::routing::post(handlers::delete_security),
        )
        .route(
            "/v1/admin/markets",
            axum::routing::get(handlers::list_markets).post(handlers::create_market),
        )
        .route(
            "/v1/admin/markets/{id}/update",
            axum::routing::post(handlers::update_market),
        )
        .route(
            "/v1/admin/corporate-actions",
            axum::routing::get(handlers::list_corporate_actions)
                .post(handlers::announce_corporate_action),
        )
        .route(
            "/v1/admin/corporate-actions/calendar",
            axum::routing::get(handlers::list_ca_calendar),
        )
        .route(
            "/v1/admin/corporate-actions/{id}/cancel",
            axum::routing::post(handlers::cancel_corporate_action),
        )
        // ── Sentin8:Cron proxy-back (DESIGN.md §10 transition worker).
        .route(
            "/v1/internal/schedules/run",
            axum::routing::post(handlers::internal_schedules_run),
        ),
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sentin8::shell::serve(
        sentin8::shell::Sentin8Shell::new("exchange8:refdata", "EXCHANGE8_REFDATA")
            .with_registration(registration::print)
            .with_state(|core| async move {
                // Corporate-action transition schedule (SDD §2.4) is
                // DECLARED in the registration's `schedules` section and
                // provisioned by `vertical enroll` - no boot-time cron
                // calls. The proxy-back execution is the worker
                // (`handlers::internal_schedules_run`), so the schedule
                // and the handler live together. No in-process scheduler.
                Ok(Arc::new(core))
            })
            .with_router(extend_router),
    )
    .await
}
