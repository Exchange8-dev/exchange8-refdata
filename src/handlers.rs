// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Ex8:RefData data handlers — securities reference-data API.
//!
//! Every handler follows the platform authorization pipeline (§5): verify
//! bearer → scope check → RLS context (same connection) → explicit
//! `WHERE env_id` (defense-in-depth) → query.
//!
//! Design: `design/sentin8-exchange8-corporate-actions-technical-design.md`
//! + data-model doc §3.1 (RD-owned `refdata.security`).

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, Row};
use uuid::Uuid;

use sentin8::shell::{err_db, AppState};

/// Scope constants for every handler in this module — single source of truth
/// for both the call sites and the authorization matrix tests (see
/// DESIGN.md §4).
pub mod scopes {
    /// `GET /v1/securities` + `GET /v1/securities/{id}` — deliberately
    /// multi-scope: order-placers (`exchange8:order:read`) may read
    /// reference data they trade against.
    pub const REFDATA_READ: &str = "exchange8:refdata:read";
    /// `POST /v1/securities` + every mutating handler below (markets,
    /// corporate actions, tick rules, holidays, books) — the single write
    /// vocabulary this service's registration declares for refdata depth.
    pub const REFDATA_WRITE: &str = "exchange8:refdata:write";
    /// Companion read scope accepted by the security list/get guards.
    pub const ORDERS_READ: &str = "exchange8:order:read";
    /// Corporate-action read surface (`GET /v1/corporate-actions`,
    /// calendar list) — the registration declares the CA vocabulary.
    pub const CA_READ: &str = "exchange8:corporate-action:read";
    /// Corporate-action mutation surface (`POST /v1/corporate-actions`,
    /// stage cancel) — kept distinct from REFDATA_WRITE per the
    /// registration's action-granular scopes.
    pub const CA_WRITE: &str = "exchange8:corporate-action:write";
    /// The cron proxy-back transition action (WORKER contract, see
    /// `worker.rs`); the mirror admits the cron workload's
    /// `sentin8:cron:write`.
    pub const CA_TRANSITION: &str = "exchange8:refdata:corporate-action:transition";
}

#[derive(Debug, Deserialize)]
pub struct ListSecuritiesQuery {
    pub symbol: Option<String>,
    pub security_type: Option<String>,
    pub market_id: Option<Uuid>,
    pub limit: Option<i64>,
}

/// Build the event/audit identity from the verified claims (handler path).
/// `idempotency_key` defaults to the row id for domain-row events.
#[allow(clippy::too_many_arguments)]
pub(crate) fn event_identity(
    env_id: Uuid,
    subject: &str,
    correlation_id: &str,
    idempotency_key: &str,
) -> crate::events::EventIdentity {
    crate::events::EventIdentity {
        env_id,
        subject_id: subject.to_string(),
        correlation_id: correlation_id.to_string(),
        idempotency_key: idempotency_key.to_string(),
    }
}

/// The correlation id from the platform middleware (`X-Sentin8-Correlation-
/// Id` is propagated by the shell's correlation middleware; a missing header
/// — direct CLI/dev calls — generates one so the envelope stays complete).
fn correlation_from(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-sentin8-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

/// JSON serialization failure mapping (a FromRow→Value snapshot is
/// infallible by construction — the mapping exists so `?` stays uniform).
fn err_db_json(e: serde_json::Error) -> (StatusCode, String) {
    err_db(e)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SecurityResponse {
    pub id: Uuid,
    pub env_id: Uuid,
    pub symbol: String,
    pub isin: Option<String>,
    pub security_type: String,
    pub currency: String,
    pub halted: bool,
    pub trading_state: String,
    pub shortable: String,
    pub market_id: Option<Uuid>,
    pub price_scale: i16,
    pub qty_scale: i16,
    pub min_size: i32,
}

/// `GET /v1/securities` — list securities (RLS-scoped).
pub async fn list_securities(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<ListSecuritiesQuery>,
) -> Result<Json<Vec<SecurityResponse>>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new("exchange8:refdata:read"),
    )
    .await?;
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let limit = q.limit.unwrap_or(100).min(500);
    let rows = sqlx::query_as::<_, SecurityResponse>(
        r#"SELECT id, env_id, symbol, isin, security_type, currency, halted,
                  trading_state, shortable, market_id, price_scale, qty_scale, min_size
             FROM refdata.security
            WHERE env_id = $1
              AND ($2::text IS NULL OR symbol = $2)
              AND ($3::text IS NULL OR security_type = $3)
              AND ($4::uuid IS NULL OR market_id = $4)
            ORDER BY symbol LIMIT $5"#,
    )
    .bind(a.env_id())
    .bind(q.symbol.as_deref())
    .bind(q.security_type.as_deref())
    .bind(q.market_id)
    .bind(limit)
    .fetch_all(&mut *ctx)
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CreateSecurityRequest {
    pub symbol: String,
    pub isin: Option<String>,
    pub security_type: String,
    pub currency: String,
    pub price_scale: Option<i16>,
    pub qty_scale: Option<i16>,
    pub min_size: Option<i32>,
}

/// `POST /v1/securities` — create a security (RLS-scoped insert).
pub async fn create_security(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateSecurityRequest>,
) -> Result<Json<SecurityResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new("exchange8:refdata:write"),
    )
    .await?;
    if req.symbol.trim().is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "symbol must not be empty".into(),
        ));
    }
    let price_scale = req.price_scale.unwrap_or(4); // equity ×10⁴ default
    let qty_scale = req.qty_scale.unwrap_or(0);
    let min_size = req.min_size.unwrap_or(1);
    let currency = req.currency.clone();
    let correlation_id = correlation_from(&headers);

    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, SecurityResponse>(
        r#"INSERT INTO refdata.security
               (env_id, symbol, isin, security_type, currency, price_scale, qty_scale, min_size)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           RETURNING id, env_id, symbol, isin, security_type, currency, halted,
                     trading_state, shortable, market_id, price_scale, qty_scale, min_size"#,
    )
    .bind(a.env_id())
    .bind(req.symbol.trim())
    .bind(req.isin.as_deref())
    .bind(req.security_type)
    .bind(&currency)
    .bind(price_scale)
    .bind(qty_scale)
    .bind(min_size)
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("symbol")) => (
            StatusCode::CONFLICT,
            format!("duplicate symbol {}", req.symbol),
        ),
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("currency")) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("unknown currency {}", currency),
        ),
        other => err_db(other),
    })?;
    // Durability pair on the SAME tx: outbox envelope (payload-complete —
    // the full row snapshot) + audit journal row.
    let identity = event_identity(a.env_id(), &a.claims.sub, &correlation_id, &row.id.to_string());
    let envelope = crate::events::security_event(
        crate::events::EVSecurity::Created,
        &serde_json::to_value(&row).map_err(err_db_json)?,
        &identity,
    );
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Create,
        "security",
        &row.id.to_string(),
        &format!("security {} created", row.symbol),
        None,
        envelope.get("snapshot").cloned(),
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

/// `GET /v1/securities/{id}` — single lookup.
pub async fn get_security(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<SecurityResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new("exchange8:refdata:read"),
    )
    .await?;
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, SecurityResponse>(
        r#"SELECT id, env_id, symbol, isin, security_type, currency, halted,
                  trading_state, shortable, market_id, price_scale, qty_scale, min_size
             FROM refdata.security
            WHERE env_id = $1 AND id = $2"#,
    )
    .bind(a.env_id())
    .bind(id)
    .fetch_optional(&mut *ctx)
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    row.map(Json)
        .ok_or((StatusCode::NOT_FOUND, "security not found".into()))
}

// ---------------------------------------------------------------------------
// Authorization matrix tests (see DESIGN.md §9). DB-free: the decision the
// handlers delegate to (`AuthContext::require_any_scope`) is exercised
// against the SAME constants the handlers require.
// ---------------------------------------------------------------------------
// Market + corporate-action + tick-rule + holiday + book-connection
// surfaces (DESIGN.md §4 mutation breadth). Same pipeline + durability
// pairs as the security path.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListMarketsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MarketResponse {
    pub id: Uuid,
    pub env_id: Uuid,
    pub code: String,
    pub name: String,
    pub mic: Option<String>,
    pub market_code: Option<String>,
    pub market_type: String,
    pub lot_size: i32,
    pub hidden_orders: bool,
    pub ebbo_enabled: bool,
    pub regulation: Option<String>,
    pub market_mode: String,
}

const MARKET_COLUMNS: &str = "id, env_id, code, name, mic, market_code, market_type, \
     lot_size, hidden_orders, ebbo_enabled, regulation, market_mode";

/// `GET /v1/markets` — list markets (RLS-scoped).
pub async fn list_markets(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<ListMarketsQuery>,
) -> Result<Json<Vec<MarketResponse>>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    read_gate(&state, &a, &headers).await?;
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let limit = q.limit.unwrap_or(100).min(500);
    let rows = sqlx::query_as::<_, MarketResponse>(&format!(
        r#"SELECT {MARKET_COLUMNS}
             FROM refdata.market
            WHERE env_id = $1
            ORDER BY code LIMIT $2"#
    ))
    .bind(a.env_id())
    .bind(limit)
    .fetch_all(&mut *ctx)
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CreateMarketRequest {
    /// The book-grouping code (`CHAR(1)`, unique per env).
    pub code: String,
    pub name: String,
    pub mic: Option<String>,
    pub market_code: Option<String>,
    pub market_type: Option<String>,
    pub lot_size: Option<i32>,
    pub hidden_orders: Option<bool>,
    pub ebbo_enabled: Option<bool>,
    pub regulation: Option<String>,
    pub market_mode: Option<String>,
}

/// `POST /v1/markets` + `POST /v1/admin/markets` — create a market.
pub async fn create_market(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateMarketRequest>,
) -> Result<Json<MarketResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    write_gate(&state, &a, &headers).await?;
    if req.code.trim().is_empty() || req.code.chars().count() > 1 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "market code must be a single character".into(),
        ));
    }
    if req.name.trim().is_empty() {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "name must not be empty".into()));
    }
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, MarketResponse>(
        r#"INSERT INTO refdata.market
               (env_id, code, name, mic, market_code, market_type, lot_size,
                hidden_orders, ebbo_enabled, regulation, market_mode)
           VALUES ($1, $2, $3, $4, $5, COALESCE($6, 'Exchange'),
                   COALESCE($7, 100), COALESCE($8, false), COALESCE($9, false),
                   $10, COALESCE($11, 'calendar'))
           RETURNING *"#,
    )
    .bind(a.env_id())
    .bind(req.code.trim())
    .bind(req.name.trim())
    .bind(req.mic.as_deref())
    .bind(req.market_code.as_deref())
    .bind(req.market_type.as_deref())
    .bind(req.lot_size)
    .bind(req.hidden_orders)
    .bind(req.ebbo_enabled)
    .bind(req.regulation.as_deref())
    .bind(req.market_mode.as_deref())
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("market")) => (
            StatusCode::CONFLICT,
            format!("duplicate market code {}", req.code),
        ),
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("market_type") || c.contains("market_mode")) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown market_type or market_mode vocabulary".into(),
        ),
        other => err_db(other),
    })?;
    emit_market_mutation(
        &mut ctx,
        a.env_id(),
        &a.claims.sub,
        &correlation_id,
        crate::events::MarketOp::Created,
        &row,
    )
    .await?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct UpdateMarketRequest {
    pub name: Option<String>,
    pub mic: Option<String>,
    pub market_code: Option<String>,
    pub market_type: Option<String>,
    pub lot_size: Option<i32>,
    pub hidden_orders: Option<bool>,
    pub ebbo_enabled: Option<bool>,
    pub regulation: Option<String>,
    pub market_mode: Option<String>,
}

/// `PUT /v1/markets/{id}` — update a market (guarded, full snapshot event).
pub async fn update_market(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(req): Json<UpdateMarketRequest>,
) -> Result<Json<MarketResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    write_gate(&state, &a, &headers).await?;
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, MarketResponse>(
        r#"UPDATE refdata.market
              SET name           = COALESCE($3, name),
                  mic            = COALESCE($4, mic),
                  market_code    = COALESCE($5, market_code),
                  market_type    = COALESCE($6, market_type),
                  lot_size       = COALESCE($7, lot_size),
                  hidden_orders  = COALESCE($8, hidden_orders),
                  ebbo_enabled   = COALESCE($9, ebbo_enabled),
                  regulation     = COALESCE($10, regulation),
                  market_mode    = COALESCE($11, market_mode)
            WHERE env_id = $1 AND id = $2
        RETURNING *"#,
    )
    .bind(a.env_id())
    .bind(id)
    .bind(req.name.as_deref())
    .bind(req.mic.as_deref())
    .bind(req.market_code.as_deref())
    .bind(req.market_type.as_deref())
    .bind(req.lot_size)
    .bind(req.hidden_orders)
    .bind(req.ebbo_enabled)
    .bind(req.regulation.as_deref())
    .bind(req.market_mode.as_deref())
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => (StatusCode::NOT_FOUND, "market not found".into()),
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("market_type") || c.contains("market_mode")) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown market_type or market_mode vocabulary".into(),
        ),
        other => err_db(other),
    })?;
    emit_market_mutation(
        &mut ctx,
        a.env_id(),
        &a.claims.sub,
        &correlation_id,
        crate::events::MarketOp::Updated,
        &row,
    )
    .await?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

/// Shared tail of the market mutation path: envelope + journal on the
/// CALLER's transaction.
async fn emit_market_mutation(
    ctx: &mut sqlx::PgConnection,
    env_id: Uuid,
    subject: &str,
    correlation_id: &str,
    op: crate::events::MarketOp,
    row: &MarketResponse,
) -> Result<(), (StatusCode, String)> {
    let identity =
        event_identity(env_id, subject, correlation_id, &row.id.to_string());
    let snapshot = serde_json::to_value(row).map_err(err_db_json)?;
    let envelope = crate::events::market_event(op, &snapshot, &identity);
    crate::events::emit_outbox(ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    let audit_op = match op {
        crate::events::MarketOp::Created => crate::events::AuditOp::Create,
        crate::events::MarketOp::Updated => crate::events::AuditOp::Update,
    };
    crate::events::journal_mutation(
        ctx,
        &identity,
        audit_op,
        "market",
        &row.id.to_string(),
        &format!("market {} {:?}", row.code, op),
        None,
        Some(snapshot),
    )
    .await
    .map_err(err_db)
}

/// Step-5 gate for the refdata read vocabulary (list/get handlers).
async fn read_gate(
    state: &Arc<AppState>,
    a: &sentin8::shell::AuthContext,
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, String)> {
    sentin8::shell::policy::require(
        &state.admin.policy,
        a,
        headers,
        sentin8::shield::EvalTarget::new(scopes::REFDATA_READ),
    )
    .await
    .map(|_| ())
}

/// Step-5 gate for the refdata write vocabulary (every mutating refdata
/// handler).
async fn write_gate(
    state: &Arc<AppState>,
    a: &sentin8::shell::AuthContext,
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, String)> {
    sentin8::shell::policy::require(
        &state.admin.policy,
        a,
        headers,
        sentin8::shield::EvalTarget::new(scopes::REFDATA_WRITE),
    )
    .await
    .map(|_| ())
}

/// Step-5 gate for the corporate-action vocabulary (CA announce/cancel).
async fn ca_gate(
    state: &Arc<AppState>,
    a: &sentin8::shell::AuthContext,
    headers: &axum::http::HeaderMap,
    write: bool,
) -> Result<(), (StatusCode, String)> {
    let action = if write {
        scopes::CA_WRITE
    } else {
        scopes::CA_READ
    };
    sentin8::shell::policy::require(
        &state.admin.policy,
        a,
        headers,
        sentin8::shield::EvalTarget::new(action),
    )
    .await
    .map(|_| ())
}

/// `PUT /v1/securities/{id}` — update mutable security fields. Halting/
/// trading-state changes are Ex8:ME's (market control) — the refdata
/// update path carries identity/precision/book fields only.
#[allow(clippy::too_many_arguments)]
pub async fn update_security(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(req): Json<UpdateSecurityRequest>,
) -> Result<Json<SecurityResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new(scopes::REFDATA_WRITE),
    )
    .await?;
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let before: Option<serde_json::Value> = sqlx::query_scalar(
        r#"SELECT to_jsonb(security) - 'env_id'
             FROM refdata.security
            WHERE env_id = $1 AND id = $2"#,
    )
    .bind(a.env_id())
    .bind(id)
    .fetch_optional(&mut *ctx)
    .await
    .map_err(err_db)?;
    let Some(before) = before else {
        return Err((StatusCode::NOT_FOUND, "security not found".into()));
    };
    let row = sqlx::query_as::<_, SecurityResponse>(
        r#"UPDATE refdata.security
              SET isin           = COALESCE($3, isin),
                  currency       = COALESCE($4, currency),
                  market_id      = COALESCE($5, market_id),
                  price_scale    = COALESCE($6, price_scale),
                  qty_scale      = COALESCE($7, qty_scale),
                  min_size       = COALESCE($8, min_size),
                  shortable      = COALESCE($9, shortable),
                  permanently_delisted = COALESCE($10, permanently_delisted)
            WHERE env_id = $1 AND id = $2
        RETURNING id, env_id, symbol, isin, security_type, currency, halted,
                  trading_state, shortable, market_id, price_scale, qty_scale, min_size"#,
    )
    .bind(a.env_id())
    .bind(id)
    .bind(req.isin.as_deref())
    .bind(req.currency.as_deref())
    .bind(req.market_id)
    .bind(req.price_scale)
    .bind(req.qty_scale)
    .bind(req.min_size)
    .bind(req.shortable.as_deref())
    .bind(req.permanently_delisted)
    .fetch_one(&mut *ctx)
    .await
    .map_err(err_db)?;
    emit_security_mutation(
        &mut ctx,
        a.env_id(),
        &a.claims.sub,
        &correlation_id,
        crate::events::EVSecurity::Updated,
        &row,
        &id.to_string(),
        Some(before),
    )
    .await?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
/// Mutable fields of the security update — `None` leaves the column.
pub struct UpdateSecurityRequest {
    /// ISO identifier.
    pub isin: Option<String>,
    /// ISO currency code (FK).
    pub currency: Option<String>,
    /// Market membership.
    pub market_id: Option<Uuid>,
    /// Price precision (×10ⁿ).
    pub price_scale: Option<i16>,
    /// Quantity precision (×10ⁿ).
    pub qty_scale: Option<i16>,
    /// Minimum order size.
    pub min_size: Option<i32>,
    /// Shortability (`Y`/`E`/`N`).
    pub shortable: Option<String>,
    /// Terminal delisting flag.
    pub permanently_delisted: Option<bool>,
}

/// `DELETE /v1/securities/{id}` — remove a security row. Blocked when the
/// security is referenced (the FK errors map to 409).
pub async fn delete_security(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new(scopes::REFDATA_WRITE),
    )
    .await?;
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let before: Option<serde_json::Value> = sqlx::query_scalar(
        r#"SELECT to_jsonb(security) - 'env_id'
             FROM refdata.security
            WHERE env_id = $1 AND id = $2"#,
    )
    .bind(a.env_id())
    .bind(id)
    .fetch_optional(&mut *ctx)
    .await
    .map_err(err_db)?;
    let Some(before) = before else {
        return Err((StatusCode::NOT_FOUND, "security not found".into()));
    };
    let result = sqlx::query(
        "DELETE FROM refdata.security WHERE env_id = $1 AND id = $2",
    )
    .bind(a.env_id())
    .bind(id)
    .execute(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint().is_some() => (
            StatusCode::CONFLICT,
            "security is referenced and cannot be deleted \
             (delist via permanently_delisted instead)"
                .into(),
        ),
        other => err_db(other),
    })?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "security not found".into()));
    }
    let identity =
        event_identity(a.env_id(), &a.claims.sub, &correlation_id, &id.to_string());
    let envelope = crate::events::security_event(
        crate::events::EVSecurity::Deleted,
        &before,
        &identity,
    );
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Delete,
        "security",
        &id.to_string(),
        &format!(
            "security {} deleted",
            before["symbol"].as_str().unwrap_or_default()
        ),
        Some(before.clone()),
        None,
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(serde_json::json!({ "id": id, "deleted": true })))
}

/// Shared tail of the security mutation path: the outbox envelope (full
/// snapshot) + the audit journal row, on the CALLER's transaction.
async fn emit_security_mutation(
    ctx: &mut sqlx::PgConnection,
    env_id: Uuid,
    subject: &str,
    correlation_id: &str,
    op: crate::events::EVSecurity,
    row: &SecurityResponse,
    row_id: &str,
    before: Option<serde_json::Value>,
) -> Result<(), (StatusCode, String)> {
    let identity = event_identity(env_id, subject, correlation_id, row_id);
    let snapshot = serde_json::to_value(row).map_err(err_db_json)?;
    let envelope = crate::events::security_event(op, &snapshot, &identity);
    crate::events::emit_outbox(ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    let audit_op = match op {
        crate::events::EVSecurity::Created => crate::events::AuditOp::Create,
        crate::events::EVSecurity::Updated => crate::events::AuditOp::Update,
        crate::events::EVSecurity::Deleted => crate::events::AuditOp::Delete,
    };
    crate::events::journal_mutation(
        ctx,
        &identity,
        audit_op,
        "security",
        row_id,
        &format!("security {} {:?}", row.symbol, op),
        before,
        Some(envelope.get("snapshot").cloned().unwrap_or_default()),
    )
    .await
    .map_err(err_db)
}

// ── Corporate actions (DESIGN.md §4 CA surface) ─────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CorporateActionResponse {
    pub id: Uuid,
    pub env_id: Uuid,
    pub security_id: Uuid,
    pub action_type: String,
    pub announce_date: Option<chrono::NaiveDate>,
    pub ex_date: Option<chrono::NaiveDate>,
    pub record_date: Option<chrono::NaiveDate>,
    pub pay_date: Option<chrono::NaiveDate>,
    pub numerator: Option<rust_decimal::Decimal>,
    pub denominator: Option<rust_decimal::Decimal>,
    pub stage: String,
}

#[derive(Debug, Deserialize)]
pub struct ListCaQuery {
    pub security_id: Option<Uuid>,
    pub stage: Option<String>,
    pub limit: Option<i64>,
}

/// `GET /v1/corporate-actions` — list corporate actions (RLS-scoped; the
/// CA lifecycle calendar view).
pub async fn list_corporate_actions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<ListCaQuery>,
) -> Result<Json<Vec<CorporateActionResponse>>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    ca_gate(&state, &a, &headers, false).await?;
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let limit = q.limit.unwrap_or(100).min(500);
    let rows = sqlx::query_as::<_, CorporateActionResponse>(
        r#"SELECT id, env_id, security_id, action_type, announce_date, ex_date,
                  record_date, pay_date, numerator, denominator, stage
             FROM refdata.corporate_action
            WHERE env_id = $1
              AND ($2::uuid IS NULL OR security_id = $2)
              AND ($3::text IS NULL OR stage = $3)
            ORDER BY announce_date NULLS LAST, id
            LIMIT $4"#,
    )
    .bind(a.env_id())
    .bind(q.security_id)
    .bind(q.stage.as_deref())
    .bind(limit)
    .fetch_all(&mut *ctx)
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct AnnounceCaRequest {
    pub security_id: Uuid,
    /// One of the schema CHECK vocabulary (`split`, `cash_dividend`, …).
    pub action_type: String,
    pub announce_date: Option<chrono::NaiveDate>,
    pub ex_date: Option<chrono::NaiveDate>,
    pub record_date: Option<chrono::NaiveDate>,
    pub pay_date: Option<chrono::NaiveDate>,
    pub numerator: Option<rust_decimal::Decimal>,
    pub denominator: Option<rust_decimal::Decimal>,
}

/// `POST /v1/corporate-actions` — announce a corporate action (stage
/// `announced`; outbox `announced` event; audit journal row).
pub async fn announce_corporate_action(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AnnounceCaRequest>,
) -> Result<Json<CorporateActionResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    ca_gate(&state, &a, &headers, true).await?;
    if req.ex_date.is_none() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "ex_date is required — the lifecycle advances on the milestone dates".into(),
        ));
    }
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, CorporateActionResponse>(
        r#"INSERT INTO refdata.corporate_action
               (env_id, security_id, action_type, announce_date, ex_date,
                record_date, pay_date, numerator, denominator, stage)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'announced')
           RETURNING id, env_id, security_id, action_type, announce_date, ex_date,
                     record_date, pay_date, numerator, denominator, stage"#,
    )
    .bind(a.env_id())
    .bind(req.security_id)
    .bind(&req.action_type)
    .bind(req.announce_date)
    .bind(req.ex_date)
    .bind(req.record_date)
    .bind(req.pay_date)
    .bind(req.numerator)
    .bind(req.denominator)
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db)
            if db
                .constraint()
                .is_some_and(|c| c.contains("action_type") || c.contains("stage")) =>
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown corporate-action type vocabulary".into(),
        ),
        sqlx::Error::Database(db)
            if db.constraint().is_some_and(|c| c.contains("security_id")) =>
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown security".into(),
        ),
        other => err_db(other),
    })?;
    emit_ca_mutation(
        &mut ctx,
        a.env_id(),
        &a.claims.sub,
        &correlation_id,
        crate::events::CaOp::Announced,
        &row,
    )
    .await?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct CancelCaRequest {
    /// Optional operator note — captured in the audit journal detail via
    /// the cancel message when provided (accepted for the operator UX).
    #[allow(dead_code)]
    pub reason: Option<String>,
}

/// `POST /v1/corporate-actions/{id}/cancel` — withdraw an action while it
/// is still `announced` (the stage predicate is the guard: a raced tick
/// that already advanced makes the cancel a 409). Events + journal in the
/// same tx.
pub async fn cancel_corporate_action(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    body: Option<Json<CancelCaRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let _ = body;
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    ca_gate(&state, &a, &headers, true).await?;
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, CorporateActionResponse>(
        r#"UPDATE refdata.corporate_action
              SET ex_date = NULL, record_date = NULL, pay_date = NULL
            WHERE env_id = $1 AND id = $2 AND stage = 'announced'
        RETURNING id, env_id, security_id, action_type, announce_date, ex_date,
                  record_date, pay_date, numerator, denominator, stage"#,
    )
    .bind(a.env_id())
    .bind(id)
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => (
            StatusCode::CONFLICT,
            "corporate action is not cancellable (already advanced or unknown)".into(),
        ),
        other => err_db(other),
    })?;
    let identity = event_identity(a.env_id(), &a.claims.sub, &correlation_id, &id.to_string());
    let snapshot = serde_json::to_value(&row).map_err(err_db_json)?;
    let envelope = crate::events::ca_event(crate::events::CaOp::Cancelled, &snapshot, &identity);
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Update,
        "corporate_action",
        &id.to_string(),
        "corporate action cancelled — milestones cleared (stage announced)",
        None,
        Some(snapshot),
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(serde_json::json!({
        "id": id, "cancelled": true, "stage": row.stage,
    })))
}

/// Shared tail of the CA mutation path: envelope + journal on the CALLER's
/// transaction.
async fn emit_ca_mutation(
    ctx: &mut sqlx::PgConnection,
    env_id: Uuid,
    subject: &str,
    correlation_id: &str,
    op: crate::events::CaOp,
    row: &CorporateActionResponse,
) -> Result<(), (StatusCode, String)> {
    let identity = event_identity(env_id, subject, correlation_id, &row.id.to_string());
    let snapshot = serde_json::to_value(row).map_err(err_db_json)?;
    let envelope = crate::events::ca_event(op, &snapshot, &identity);
    crate::events::emit_outbox(ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    let (audit_op, message) = match op {
        crate::events::CaOp::Announced => (
            crate::events::AuditOp::Create,
            format!(
                "corporate action {} ({}) announced — ex_date {}",
                row.id,
                row.action_type,
                row.ex_date.map(|d| d.to_string()).unwrap_or_default()
            ),
        ),
        crate::events::CaOp::StageAdvanced => (
            crate::events::AuditOp::Execute,
            format!("corporate action {} advanced to {}", row.id, row.stage),
        ),
        crate::events::CaOp::Completed => (
            crate::events::AuditOp::Execute,
            format!("corporate action {} completed (pay_date)", row.id),
        ),
        crate::events::CaOp::Cancelled => (
            crate::events::AuditOp::Update,
            format!("corporate action {} cancelled", row.id),
        ),
    };
    crate::events::journal_mutation(
        ctx,
        &identity,
        audit_op,
        "corporate_action",
        &row.id.to_string(),
        &message,
        None,
        Some(snapshot),
    )
    .await
    .map_err(err_db)
}

/// `GET /v1/corporate-actions/calendar` — the upcoming-action view
/// (calendar rows joined to their actions; CA read scope).
pub async fn list_ca_calendar(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    ca_gate(&state, &a, &headers, false).await?;
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let rows = sqlx::query(
        r#"SELECT c.id, c.action_id, c.effective_date, a.action_type, a.stage, a.security_id
             FROM refdata.corporate_action_calendar c
             JOIN refdata.corporate_action a ON a.id = c.action_id
            WHERE c.env_id = $1
            ORDER BY c.effective_date LIMIT 500"#,
    )
    .bind(a.env_id())
    .fetch_all(&mut *ctx)
    .await
    .map_err(err_db)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: Uuid = r.try_get("id").map_err(err_db)?;
        let action_id: Uuid = r.try_get("action_id").map_err(err_db)?;
        let effective_date: chrono::NaiveDate = r.try_get("effective_date").map_err(err_db)?;
        let action_type: String = r.try_get("action_type").map_err(err_db)?;
        let stage: String = r.try_get("stage").map_err(err_db)?;
        let security_id: Uuid = r.try_get("security_id").map_err(err_db)?;
        out.push(serde_json::json!({
            "id": id, "action_id": action_id, "effective_date": effective_date,
            "action_type": action_type, "stage": stage, "security_id": security_id,
        }));
    }
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(out))
}

// ── Tick rules, holidays, books (DESIGN.md §4 mutation breadth) ─────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TickRuleResponse {
    pub id: Uuid,
    pub env_id: Uuid,
    pub name: String,
}

/// `GET /v1/tick-rules` — list tick rules.
pub async fn list_tick_rules(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<TickRuleResponse>>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    read_gate(&state, &a, &headers).await?;
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let rows = sqlx::query_as::<_, TickRuleResponse>(
        "SELECT id, env_id, name FROM refdata.tick_rule WHERE env_id = $1 ORDER BY name LIMIT 500",
    )
    .bind(a.env_id())
    .fetch_all(&mut *ctx)
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CreateTickRuleRequest {
    pub name: String,
    /// Bands: (min_price, max_price, tick) rows.
    pub bands: Vec<TickBandRequest>,
}

#[derive(Debug, Deserialize)]
pub struct TickBandRequest {
    pub min_price: rust_decimal::Decimal,
    pub max_price: Option<rust_decimal::Decimal>,
    pub tick_value: rust_decimal::Decimal,
}

/// `POST /v1/tick-rules` — create a tick rule + its bands in one tx
/// (outbox `tick-rule:created` + audit journal row).
pub async fn create_tick_rule(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateTickRuleRequest>,
) -> Result<Json<TickRuleResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    write_gate(&state, &a, &headers).await?;
    if req.name.trim().is_empty() {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "name must not be empty".into()));
    }
    if req.bands.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "at least one tick band is required".into(),
        ));
    }
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, TickRuleResponse>(
        r#"INSERT INTO refdata.tick_rule (env_id, name)
           VALUES ($1, $2)
           RETURNING id, env_id, name"#,
    )
    .bind(a.env_id())
    .bind(req.name.trim())
    .fetch_one(&mut *ctx)
    .await
    .map_err(err_db)?;
    for b in &req.bands {
        sqlx::query(
            r#"INSERT INTO refdata.tick_rule_value
                   (env_id, rule_id, min_price, max_price, tick_value)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(a.env_id())
        .bind(row.id)
        .bind(b.min_price)
        .bind(b.max_price)
        .bind(b.tick_value)
        .execute(&mut *ctx)
        .await
        .map_err(err_db)?;
    }
    let identity = event_identity(a.env_id(), &a.claims.sub, &correlation_id, &row.id.to_string());
    let envelope = crate::events::simple_event(
        crate::events::EV_TICK_RULE_CREATED,
        &identity,
        serde_json::json!({ "id": row.id, "name": row.name, "env_id": a.env_id() }),
    );
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Create,
        "tick_rule",
        &row.id.to_string(),
        &format!("tick rule {} created ({} bands)", row.name, req.bands.len()),
        None,
        Some(envelope.get("snapshot").cloned().unwrap_or_default()),
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct UpdateTickRuleRequest {
    pub name: Option<String>,
}

/// `PUT /v1/tick-rules/{id}` — rename a tick rule (band edits are separate
/// writes under the same write scope; renames are the admin-surface
/// mutation).
pub async fn update_tick_rule(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(req): Json<UpdateTickRuleRequest>,
) -> Result<Json<TickRuleResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    write_gate(&state, &a, &headers).await?;
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, TickRuleResponse>(
        r#"UPDATE refdata.tick_rule
              SET name = COALESCE($3, name)
            WHERE env_id = $1 AND id = $2
        RETURNING id, env_id, name"#,
    )
    .bind(a.env_id())
    .bind(id)
    .bind(req.name.as_deref())
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => (StatusCode::NOT_FOUND, "tick rule not found".into()),
        other => err_db(other),
    })?;
    let identity = event_identity(a.env_id(), &a.claims.sub, &correlation_id, &id.to_string());
    let envelope = crate::events::simple_event(
        crate::events::EV_TICK_RULE_UPDATED,
        &identity,
        serde_json::json!({ "id": row.id, "name": row.name, "env_id": a.env_id() }),
    );
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Update,
        "tick_rule",
        &id.to_string(),
        &format!("tick rule {} renamed to {}", id, row.name),
        None,
        Some(serde_json::json!({ "id": id, "name": row.name })),
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct CreateHolidayRequest {
    pub holiday_date: chrono::NaiveDate,
    pub name: String,
    /// Markets the holiday closes (empty = the whole venue).
    pub market_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct HolidayResponse {
    pub id: Uuid,
    pub env_id: Uuid,
    pub holiday_date: chrono::NaiveDate,
    pub name: String,
}

/// `POST /v1/holidays` — declare a market holiday (+ per-market closure
/// rows; outbox `holiday:created` + audit journal row).
pub async fn create_holiday(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateHolidayRequest>,
) -> Result<Json<HolidayResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    write_gate(&state, &a, &headers).await?;
    if req.name.trim().is_empty() {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "name must not be empty".into()));
    }
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, HolidayResponse>(
        r#"INSERT INTO refdata.holiday (env_id, holiday_date, name)
           VALUES ($1, $2, $3)
           RETURNING id, env_id, holiday_date, name"#,
    )
    .bind(a.env_id())
    .bind(req.holiday_date)
    .bind(req.name.trim())
    .fetch_one(&mut *ctx)
    .await
    .map_err(err_db)?;
    for market_id in &req.market_ids {
        sqlx::query(
            r#"INSERT INTO refdata.holiday_market (env_id, holiday_id, market_id)
               VALUES ($1, $2, $3)"#,
        )
        .bind(a.env_id())
        .bind(row.id)
        .bind(market_id)
        .execute(&mut *ctx)
        .await
        .map_err(err_db)?;
    }
    let identity = event_identity(a.env_id(), &a.claims.sub, &correlation_id, &row.id.to_string());
    let envelope = crate::events::simple_event(
        crate::events::EV_HOLIDAY_CREATED,
        &identity,
        serde_json::json!({
            "id": row.id, "holiday_date": row.holiday_date, "name": row.name,
            "markets": req.market_ids, "env_id": a.env_id(),
        }),
    );
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Create,
        "holiday",
        &row.id.to_string(),
        &format!("holiday {} declared", row.name),
        None,
        Some(envelope.get("snapshot").cloned().unwrap_or_default()),
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct CreateBookConnectionRequest {
    pub book_id: Uuid,
    /// GW-owned connection comp id.
    pub connection_id: String,
    pub seq: Option<i32>,
    /// `E` (enabled) or `D` (disabled).
    pub status: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BookConnectionResponse {
    pub id: Uuid,
    pub env_id: Uuid,
    pub book_id: Uuid,
    pub connection_id: String,
    pub seq: i32,
    pub status: String,
}

/// `POST /v1/books/connections` — register a book↔connection membership
/// (outbox `book-connection:created` + audit journal row).
pub async fn create_book_connection(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateBookConnectionRequest>,
) -> Result<Json<BookConnectionResponse>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    write_gate(&state, &a, &headers).await?;
    if req.connection_id.trim().is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "connection_id must not be empty".into(),
        ));
    }
    let correlation_id = correlation_from(&headers);
    let mut conn = state.pool.acquire().await.map_err(err_db)?;
    let mut ctx = conn.begin().await.map_err(err_db)?;
    sentin8::shell::set_tenant_context(&mut ctx, a.env_id()).await;
    let row = sqlx::query_as::<_, BookConnectionResponse>(
        r#"INSERT INTO refdata.book_connection
               (env_id, book_id, connection_id, seq, status)
           VALUES ($1, $2, $3, COALESCE($4, 1), COALESCE($5, 'E'))
           RETURNING id, env_id, book_id, connection_id, seq, status"#,
    )
    .bind(a.env_id())
    .bind(req.book_id)
    .bind(req.connection_id.trim())
    .bind(req.seq)
    .bind(req.status.as_deref())
    .fetch_one(&mut *ctx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("status")) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "book_connection status must be 'E' or 'D'".into(),
        ),
        sqlx::Error::Database(db) if db.constraint().is_some_and(|c| c.contains("book_id")) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown book".into(),
        ),
        other => err_db(other),
    })?;
    let identity = event_identity(a.env_id(), &a.claims.sub, &correlation_id, &row.id.to_string());
    let snapshot = serde_json::to_value(&row).map_err(err_db_json)?;
    let envelope = crate::events::simple_event(
        crate::events::EV_BOOK_CONNECTION_CREATED,
        &identity,
        snapshot,
    );
    crate::events::emit_outbox(&mut ctx, &identity, &envelope)
        .await
        .map_err(err_db)?;
    crate::events::journal_mutation(
        &mut ctx,
        &identity,
        crate::events::AuditOp::Create,
        "book_connection",
        &row.id.to_string(),
        &format!(
            "book connection {} registered (book {})",
            row.connection_id, row.book_id
        ),
        None,
        Some(serde_json::to_value(&row).map_err(err_db_json)?),
    )
    .await
    .map_err(err_db)?;
    ctx.commit().await.map_err(err_db)?;
    Ok(Json(row))
}

// ── Admin RFW page surface (DESIGN.md §4) ───────────────────────────────

/// `GET /v1/admin/ui/{page}` — the RFW page definition (Dash renders it).
/// Gated on the page's declared Shield action — UI definitions carry no
/// tenant data; the decision is the step-5 mirror evaluation (the
/// platform-admin trio rides the live evaluation). Returns 404 for an
/// undeclared page.
pub async fn admin_ui_page(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(page): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    let definition = crate::admin_ui::refdata_pages()
        .into_iter()
        .find(|p| p.slug == page)
        .ok_or((StatusCode::NOT_FOUND, "unknown admin page".into()))?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new(definition.scope),
    )
    .await
    .map(|_| ())?;
    Ok(Json(definition.to_rfw()))
}

// ── Cron proxy-back (DESIGN.md §2 request family 3, §10 worker) ─────────

#[derive(Debug, Deserialize)]
pub struct ScheduleRunRequest {
    pub schedule_id: Option<Uuid>,
    pub schedule_type: String,
    /// The env the cron row carries — the transition scope.
    pub env_id: Uuid,
    pub correlation_id: Option<String>,
    /// The cron ticker's job idempotency key (`schedule:{id}:{job}`) —
    /// carried through to the audit trail; the stage transitions dedupe on
    /// their own (action id, target stage) key.
    #[allow(dead_code)]
    pub idempotency_key: Option<String>,
}

/// `POST /v1/internal/schedules/run` — Sentin8:Cron proxy-back target for
/// the corporate-action transition worker (the platform secrets
/// precedent: the ticker calls back with its own workload certificate,
/// verified like any other principal; step 5 evaluates the dedicated
/// transition action and the mirror admits the cron workload's
/// `sentin8:cron:write`).
///
/// Returns `{ advanced }` for the schedule's env. A 500 is a DB failure —
/// cron retries with backoff and the stage-predicated UPDATE re-runs
/// (idempotent on `(action id, target stage)`).
pub async fn internal_schedules_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ScheduleRunRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let a = sentin8::shell::verify_token(&state.config, &headers).await?;
    sentin8::shell::policy::require(
        &state.admin.policy,
        &a,
        &headers,
        sentin8::shield::EvalTarget::new(scopes::CA_TRANSITION),
    )
    .await
    .map(|_| ())?;
    let correlation_id = body
        .correlation_id
        .clone()
        .unwrap_or_else(|| body.schedule_id.map(|s| s.to_string()).unwrap_or_default());
    let advanced = crate::worker::advance_due_transitions(
        &state,
        body.env_id,
        &a.claims.sub,
        &correlation_id,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "schedule_type": body.schedule_type,
        "env_id": body.env_id,
        "advanced": advanced,
    })))
}

// ---------------------------------------------------------------------------
// Authorization matrix tests (see DESIGN.md §9). DB-free: the decision the
// handlers delegate to (`AuthContext::require_any_scope`) is exercised
// against the SAME constants the handlers require.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod scope_matrix {
    use super::scopes;
    use sentin8::shell::{AuthContext, VerifiedClaims};

    const HANDLER_SCOPES: &[(&str, &[&str])] = &[
        (
            "list_securities",
            &[scopes::REFDATA_READ, scopes::ORDERS_READ],
        ),
        ("create_security", &[scopes::REFDATA_WRITE]),
        ("get_security", &[scopes::REFDATA_READ, scopes::ORDERS_READ]),
        ("update_security", &[scopes::REFDATA_WRITE]),
        ("delete_security", &[scopes::REFDATA_WRITE]),
        ("list_markets", &[scopes::REFDATA_READ, scopes::ORDERS_READ]),
        ("create_market", &[scopes::REFDATA_WRITE]),
        ("update_market", &[scopes::REFDATA_WRITE]),
        ("list_corporate_actions", &[scopes::CA_READ]),
        ("announce_corporate_action", &[scopes::CA_WRITE]),
        ("cancel_corporate_action", &[scopes::CA_WRITE]),
        ("list_ca_calendar", &[scopes::CA_READ]),
        ("list_tick_rules", &[scopes::REFDATA_READ, scopes::ORDERS_READ]),
        ("create_tick_rule", &[scopes::REFDATA_WRITE]),
        ("update_tick_rule", &[scopes::REFDATA_WRITE]),
        ("create_holiday", &[scopes::REFDATA_WRITE]),
        ("create_book_connection", &[scopes::REFDATA_WRITE]),
        ("internal_schedules_run", &[scopes::CA_TRANSITION]),
    ];

    const SIBLING_SCOPES: &[&str] = &[
        scopes::REFDATA_READ,
        scopes::REFDATA_WRITE,
        scopes::ORDERS_READ,
        scopes::CA_READ,
        scopes::CA_WRITE,
        scopes::CA_TRANSITION,
    ];

    fn ctx(scope: &str) -> AuthContext {
        AuthContext {
            claims: VerifiedClaims {
                sub: "matrix-test".into(),
                iss: "https://auth.test".into(),
                aud: "exchange8".into(),
                jti: "matrix-jti".into(),
                iat: 0,
                exp: i64::MAX,
                token_tp: "access".into(),
                scope: scope.into(),
                env_id: Some("00000000-0000-0000-0000-000000000002".into()),
                email: None,
                act: None,
                vertical: None,
                service_type: None,
            },
            // Bearer-path context: no certificate workload on the test
            // identity (CERTFREE-4V — the field exists for the cert path).
            workload: None,
        }
    }

    #[test]
    fn every_handler_denies_without_and_with_wrong_scope() {
        for (handler, required) in HANDLER_SCOPES {
            let err = ctx("")
                .require_any_scope(required)
                .expect_err("no scope must deny");
            assert_eq!(err.0.as_u16(), 403, "{handler}: expected FORBIDDEN");

            let err = ctx("bank8:ledger:read")
                .require_any_scope(required)
                .expect_err("foreign scope must deny");
            assert_eq!(err.0.as_u16(), 403, "{handler}: foreign must be FORBIDDEN");

            for sibling in SIBLING_SCOPES {
                if required.contains(sibling) {
                    continue;
                }
                let err = ctx(sibling)
                    .require_any_scope(required)
                    .expect_err("sibling scope must deny");
                assert_eq!(
                    err.0.as_u16(),
                    403,
                    "{handler}: sibling {sibling} must be FORBIDDEN"
                );
            }
        }
    }

    #[test]
    fn every_handler_allows_each_required_scope_and_platform_admin() {
        for (handler, required) in HANDLER_SCOPES {
            for scope in *required {
                assert!(
                    ctx(scope).require_any_scope(required).is_ok(),
                    "{handler}: exact scope {scope} must allow"
                );
            }
            let all = required.join(" ");
            assert!(ctx(&all).require_any_scope(required).is_ok(), "{handler}");
            assert!(
                ctx("sentin8:sys:admin").require_any_scope(required).is_ok(),
                "{handler}"
            );
        }
    }

    #[test]
    fn order_placers_can_read_securities_without_refdata_scope() {
        // The multi-scope guard is deliberate: a session that may place
        // orders but holds no refdata scope still reads the securities it
        // trades against. `has_any_scope` semantics = ANY required scope.
        assert!(ctx(scopes::ORDERS_READ)
            .require_any_scope(&[scopes::REFDATA_READ, scopes::ORDERS_READ])
            .is_ok());
        // …but write stays write-gated: a reader cannot create securities.
        assert!(ctx(scopes::ORDERS_READ)
            .require_any_scope(&[scopes::REFDATA_WRITE])
            .is_err());
    }

    #[test]
    fn grants_are_prefix_permissive_and_denials_are_exact() {
        assert!(ctx("exchange8:refdata:read:all")
            .require_any_scope(&[scopes::REFDATA_READ])
            .is_ok());
        assert!(ctx("exchange8:refdata:readx")
            .require_any_scope(&[scopes::REFDATA_READ])
            .is_err());
    }

    #[test]
    fn scope_constants_follow_the_naming_convention() {
        // Registered contract: 3-4 segments (see clearing scope_matrix note).
        let mut all: Vec<&str> = Vec::new();
        for (handler, required) in HANDLER_SCOPES {
            for scope in *required {
                all.push(scope);
                let segs: Vec<_> = scope.split(':').collect();
                assert!(
                    (3..=4).contains(&segs.len()),
                    "{handler}: {scope} must have 3-4 segments"
                );
                assert_eq!(segs[0], "exchange8", "{handler}: {scope} wrong vertical");
                assert!(
                    segs.iter().all(|s| !s.is_empty()
                        && s.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')),
                    "{handler}: {scope} segments must be lowercase alnum (+hyphen)"
                );
            }
        }
        // Every declared constant must be wired into HANDLER_SCOPES. Shared
        // constants across handlers are legitimate — dedup first.
        all.sort_unstable();
        all.dedup();
        let mut declared = vec![
            scopes::REFDATA_READ,
            scopes::REFDATA_WRITE,
            scopes::ORDERS_READ,
            scopes::CA_READ,
            scopes::CA_WRITE,
            scopes::CA_TRANSITION,
        ];
        declared.sort_unstable();
        assert_eq!(all, declared, "scopes module and HANDLER_SCOPES diverged");
    }
}
