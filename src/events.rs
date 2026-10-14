// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Ex8:RefData typed event envelopes (DBX producer path).
//!
//! Every mutating handler writes an outbox row in the SAME transaction as
//! the domain change with a **payload-complete** envelope (event-first rule
//! — the consumer reacts from the payload alone, no callback): `env_id`,
//! subject, correlation/idempotency ids, and the full domain-row snapshot.
//! The shell's outbox relay (its own boot step 8b) pushes the rows to
//! Sentin8:Bus `POST /v1/produce` and marks them relayed.
//!
//! Stream family: `exchange8` — the Bus stream key is
//! `bus:{env_id}:{family}`; the family slug is validated by
//! `sentin8::bus::outbox::validate_stream_family` (colons are structurally
//! impossible), matching the sibling convention (`gateway`, `me` leaves).
//! Event NAMES carry the vertical identity:
//! `exchange8:refdata:{resource}:{operation}` (colon format throughout).
//!
//! Design: `services/refdata/DESIGN.md` §4 (event flow), §10.

use serde_json::{json, Value};
use uuid::Uuid;

/// Write the outbox row for an envelope in the CALLER's transaction
/// (same tx as the domain change — outbox-first). The relay (shell boot
/// step 8b) pushes it to Bus post-commit; failure of the write aborts the
/// caller's tx via `?` at the call site. `publisher_subject` carries the
/// request's verified `sub` so consumers can attribute the event.
///
/// # Errors
/// Propagates the SQL error (the validator's rejection of the constant
/// family text is structurally impossible — covered by the registry test).
pub async fn emit_outbox(
    tx: &mut sqlx::PgConnection,
    identity: &EventIdentity,
    envelope: &Value,
) -> Result<Uuid, sqlx::Error> {
    let ev = envelope["event_type"].as_str().unwrap_or_default();
    match sentin8::bus::outbox::insert_outbox_row_with_publisher(
        &mut *tx,
        identity.env_id,
        STREAM_FAMILY,
        ev,
        envelope,
        None,
        Some(&identity.subject_id),
    )
    .await
    {
        Ok(id) => Ok(id),
        Err(sentin8::bus::outbox::OutboxError::Db(e)) => Err(e),
        Err(e) => Err(sqlx::Error::Configuration(Box::new(e))),
    }
}

/// Stream family for all refdata-emitted events.
///
/// Bare service slug — Bus `XADD bus:{env}:{family}`; colons are
/// structurally impossible per `sentin8::bus::outbox::validate_stream_family`
/// (the gateway module covers the same convention, see DESIGN.md §4).
pub const STREAM_FAMILY: &str = "exchange8";

/// `exchange8:refdata:security:created` — a security row was inserted.
pub const EV_SECURITY_CREATED: &str = "exchange8:refdata:security:created";
/// `exchange8:refdata:security:updated` — a security row was updated.
pub const EV_SECURITY_UPDATED: &str = "exchange8:refdata:security:updated";
/// `exchange8:refdata:security:deleted` — a security row was deleted.
pub const EV_SECURITY_DELETED: &str = "exchange8:refdata:security:deleted";
/// `exchange8:refdata:market:created` — a market row was inserted.
pub const EV_MARKET_CREATED: &str = "exchange8:refdata:market:created";
/// `exchange8:refdata:market:updated` — a market row was updated.
pub const EV_MARKET_UPDATED: &str = "exchange8:refdata:market:updated";
/// `exchange8:refdata:corporate-action:announced` — CA row inserted
/// (stage `announced`).
pub const EV_CA_ANNOUNCED: &str = "exchange8:refdata:corporate-action:announced";
/// `exchange8:refdata:corporate-action:stage-advanced` — the lifecycle
/// moved one stage forward (cron transition worker).
pub const EV_CA_STAGE_ADVANCED: &str = "exchange8:refdata:corporate-action:stage-advanced";
/// `exchange8:refdata:corporate-action:completed` — the CA reached
/// `pay_date` (terminal stage; delivery handoff to Ex8:Settlement).
pub const EV_CA_COMPLETED: &str = "exchange8:refdata:corporate-action:completed";
/// `exchange8:refdata:corporate-action:cancelled` — a CA was withdrawn
/// by an operator before completion.
pub const EV_CA_CANCELLED: &str = "exchange8:refdata:corporate-action:cancelled";
/// `exchange8:refdata:tick-rule:created` — a tick rule row was inserted.
pub const EV_TICK_RULE_CREATED: &str = "exchange8:refdata:tick-rule:created";
/// `exchange8:refdata:tick-rule:updated` — a tick rule row was updated.
pub const EV_TICK_RULE_UPDATED: &str = "exchange8:refdata:tick-rule:updated";
/// `exchange8:refdata:book-connection:created` — a book↔connection
/// membership row was inserted.
pub const EV_BOOK_CONNECTION_CREATED: &str = "exchange8:refdata:book-connection:created";
/// `exchange8:refdata:holiday:created` — a holiday row was inserted.
pub const EV_HOLIDAY_CREATED: &str = "exchange8:refdata:holiday:created";

/// The identity carried inside every envelope (handler-owned, never derived
/// from DB or headers — event-first: consumers re-key their caches without
/// callbacks).
pub struct EventIdentity {
    /// Tenant environment of the event (the JWT `env_id` for HTTP handlers,
    /// the cron row's env for the transitions worker).
    pub env_id: Uuid,
    /// Acting subject (JWT `sub`; the cron proxy-back's M2M client id).
    pub subject_id: String,
    /// Correlation id (from `X-Sentin8-Correlation-Id` when present —
    /// otherwise generated; for cron fires the schedule id).
    pub correlation_id: String,
    /// Idempotency key: consumers dedupe on this (stage transitions dedupe
    /// on `(corporate_action id, target stage)`).
    pub idempotency_key: String,
}

/// Envelope for a security mutation: the FULL row snapshot (the exact
/// `SecurityResponse` fields) — consumers (ME, vfeeds, RFQ) re-key their
/// caches without a read-back.
pub fn security_event(ev: EVSecurity, sec: &serde_json::Value, identity: &EventIdentity) -> Value {
    json!({
        "event_type": ev_name(ev),
        "env_id": identity.env_id,
        "subject_id": identity.subject_id,
        "correlation_id": identity.correlation_id,
        "idempotency_key": identity.idempotency_key,
        "snapshot": sec,
    })
}

/// Which security mutation produced the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EVSecurity {
    /// Inserted.
    Created,
    /// Updated.
    Updated,
    /// Deleted.
    Deleted,
}

/// Envelope for a market mutation (`snapshot` = the full row).
pub fn market_event(operation: MarketOp, market: &Value, identity: &EventIdentity) -> Value {
    json!({
        "event_type": match operation {
            MarketOp::Created => EV_MARKET_CREATED,
            MarketOp::Updated => EV_MARKET_UPDATED,
        },
        "env_id": identity.env_id,
        "subject_id": identity.subject_id,
        "correlation_id": identity.correlation_id,
        "idempotency_key": identity.idempotency_key,
        "snapshot": market,
    })
}

/// Which market mutation produced the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketOp {
    /// Inserted.
    Created,
    /// Updated.
    Updated,
}

/// Envelope for a corporate-action mutation: ids + ratio + dates + stage
/// (the full row snapshot) — the transition consumers advance their own
/// lifecycle view without callbacks.
pub fn ca_event(operation: CaOp, action: &Value, identity: &EventIdentity) -> Value {
    json!({
        "event_type": match operation {
            CaOp::Announced => EV_CA_ANNOUNCED,
            CaOp::StageAdvanced => EV_CA_STAGE_ADVANCED,
            CaOp::Completed => EV_CA_COMPLETED,
            CaOp::Cancelled => EV_CA_CANCELLED,
        },
        "env_id": identity.env_id,
        "subject_id": identity.subject_id,
        "correlation_id": identity.correlation_id,
        "idempotency_key": identity.idempotency_key,
        "snapshot": action,
    })
}

/// Which corporate-action mutation produced the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaOp {
    /// Row inserted (stage `announced`).
    Announced,
    /// Lifecycle advanced one stage (cron transition worker).
    StageAdvanced,
    /// Reaching the terminal `pay_date` stage (Settlement handoff).
    Completed,
    /// Withdrawn by an operator before completion.
    Cancelled,
}

/// Envelope with the caller-provided snapshot for the smaller surfaces
/// (tick rule, holiday, book connection) — same completeness contract,
/// fewer columns to project.
pub fn simple_event(event_type: &'static str, identity: &EventIdentity, snapshot: Value) -> Value {
    json!({
        "event_type": event_type,
        "env_id": identity.env_id,
        "subject_id": identity.subject_id,
        "correlation_id": identity.correlation_id,
        "idempotency_key": identity.idempotency_key,
        "snapshot": snapshot,
    })
}

fn ev_name(ev: EVSecurity) -> &'static str {
    match ev {
        EVSecurity::Created => EV_SECURITY_CREATED,
        EVSecurity::Updated => EV_SECURITY_UPDATED,
        EVSecurity::Deleted => EV_SECURITY_DELETED,
    }
}

// ── Audit journal (architecture-principles §6.8) ─────────────────────────

/// Audit `action` words accepted by [`audit_event`] (maps one-to-one to
/// `sentin8_audit::AuditAction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditOp {
    /// Resource created.
    Create,
    /// Resource modified.
    Update,
    /// Resource deleted.
    Delete,
    /// Lifecycle executed (stage transition).
    Execute,
}

/// Build the audit envelope for one mutation and write the
/// `refdata.audit_journal` row on the CALLER's transaction (same tx as the
/// domain change; the shell's pusher — boot step 8c — drains it to
/// Sentin8:Sys `POST /v1/audit` with the `refdata-audit` M2M workload).
/// Reads are NOT journalized (audit-journal pattern: state changes only).
pub async fn journal_mutation(
    tx: &mut sqlx::PgConnection,
    identity: &EventIdentity,
    op: AuditOp,
    resource_type: &str,
    resource_id: &str,
    message: &str,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), sqlx::Error> {
    use sentin8::audit::{AuditAction, AuditEvent, AuditSeverity};
    let action = match op {
        AuditOp::Create => AuditAction::Create,
        AuditOp::Update => AuditAction::Update,
        AuditOp::Delete => AuditAction::Delete,
        AuditOp::Execute => AuditAction::Execute,
    };
    let ev = AuditEvent::builder()
        .service("exchange8:refdata")
        .action(action)
        .resource(resource_type, resource_id)
        .actor(&identity.subject_id, "machine")
        .tenant(Uuid::nil(), identity.env_id)
        .severity(AuditSeverity::Info)
        .correlation_id(&identity.correlation_id)
        .message(message);
    let ev = match before {
        Some(b) => ev.before(b),
        None => ev,
    };
    let ev = match after {
        Some(aft) => ev.after(aft),
        None => ev,
    };
    sentin8::audit::journal_insert(&mut *tx, &ev.build()).await
}

/// Registry test coverage (DESIGN.md §9): every declared event name uses
/// the platform event format (`vertical:service:resource:operation`, all
/// colons — no dots) and the stream family validates.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_follow_the_colon_format() {
        const NAMES: &[&str] = &[
            EV_SECURITY_CREATED,
            EV_SECURITY_UPDATED,
            EV_SECURITY_DELETED,
            EV_MARKET_CREATED,
            EV_MARKET_UPDATED,
            EV_CA_ANNOUNCED,
            EV_CA_STAGE_ADVANCED,
            EV_CA_COMPLETED,
            EV_CA_CANCELLED,
            EV_TICK_RULE_CREATED,
            EV_TICK_RULE_UPDATED,
            EV_BOOK_CONNECTION_CREATED,
            EV_HOLIDAY_CREATED,
        ];
        for n in NAMES {
            // Format contract: `vertical:service:resource:operation` —
            // all-colon segments, no dots. Hyphenated segments (the
            // corporate-action vocabulary) are the platform's scope convention.
            let segs: Vec<&str> = n.split(':').collect();
            assert_eq!(segs.len(), 4, "{n}: must have 4 segments");
            assert_eq!(segs[0], "exchange8", "{n}: wrong vertical");
            assert_eq!(segs[1], "refdata", "{n}: wrong service");
            assert_eq!(n.find('.'), None, "{n}: dots are not event format");
            let lower = |s: &str| {
                s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            };
            assert!(segs.iter().all(|s| !s.is_empty() && lower(s)), "{n}");
        }
    }

    #[test]
    fn stream_family_validator_safe() {
        assert!(
            sentin8::bus::outbox::validate_stream_family(STREAM_FAMILY).is_ok(),
            "colons are structurally impossible in a stream family"
        );
        assert!(sentin8::bus::outbox::validate_stream_family("refdata:security").is_err());
    }

    #[test]
    fn ca_envelope_is_payload_complete() {
        let snap = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "security_id": "00000000-0000-0000-0000-000000000002",
            "stage": "ex_date",
            "numerator": 2,
            "denominator": 1,
        });
        let identity = EventIdentity {
            env_id: uuid::Uuid::nil(),
            subject_id: "cron".into(),
            correlation_id: "sched-1".into(),
            idempotency_key: "sched-1".into(),
        };
        let v = ca_event(CaOp::StageAdvanced, &snap, &identity);
        assert_eq!(v["event_type"], EV_CA_STAGE_ADVANCED);
        assert_eq!(v["env_id"], uuid::Uuid::nil().to_string());
        assert_eq!(v["subject_id"], "cron");
        assert_eq!(v["correlation_id"], "sched-1");
        assert_eq!(v["idempotency_key"], "sched-1");
        assert_eq!(v["snapshot"]["stage"], "ex_date");
        assert!(v.get("snapshot").is_some(), "full snapshot present");
        assert!(v.pointer("/snapshot/numerator").is_some());
    }

    #[test]
    fn security_envelope_carries_the_snapshot() {
        let snap = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "symbol": "EX8A",
            "price_scale": 4,
        });
        let identity = EventIdentity {
            env_id: uuid::Uuid::nil(),
            subject_id: "user".into(),
            correlation_id: "c".into(),
            idempotency_key: "i".into(),
        };
        for (op, expected) in [
            (EVSecurity::Created, EV_SECURITY_CREATED),
            (EVSecurity::Updated, EV_SECURITY_UPDATED),
            (EVSecurity::Deleted, EV_SECURITY_DELETED),
        ] {
            let v = security_event(op, &snap, &identity);
            assert_eq!(v["event_type"], expected);
            assert_eq!(v["snapshot"]["symbol"], "EX8A");
        }
    }

    #[test]
    fn market_envelope_ops_map() {
        let snap = serde_json::json!({ "code": "A", "lot_size": 100 });
        let identity = EventIdentity {
            env_id: uuid::Uuid::nil(),
            subject_id: "user".into(),
            correlation_id: "c".into(),
            idempotency_key: "i".into(),
        };
        let created = market_event(MarketOp::Created, &snap, &identity);
        let updated = market_event(MarketOp::Updated, &snap, &identity);
        assert_eq!(created["event_type"], EV_MARKET_CREATED);
        assert_eq!(updated["event_type"], EV_MARKET_UPDATED);
    }
}
