// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
// SPDX-License-Identifier: Apache-2.0

//! Exchange8:RefData corporate-action transition worker (Sentin8:Cron-driven).
//!
//! Stage transitions are executed by Sentin8:Cron by design — the design
//! doc (`services/refdata/DESIGN.md` §2, request family 3; master design
//! `design/sentin8-exchange8-refdata-technical-design.md` §4.1) and the
//! platform precedent (`sentin8/services/secrets` — the `<leaf>-cron` M2M
//! workload + a `POST /v1/internal/schedules/run` proxy-back target) fix
//! the shape:
//!
//! 1. **Boot registration** — ensure the `refdata-cron` workload
//!    certificate (CERTFREE-4V — scope `sentin8:cron:write` comes from
//!    the registry map; grant declared in
//!    `registration.rs::M2M_GRANTS`), then create the recurring schedule
//!    `refdata-ca-transitions` at Cron → idempotent per boot (a Cron outage
//!    only delays registration; a re-run re-registers the same named-slug
//!    domain+type row — Cron's ticker dedupes the fired job per tick).
//! 2. **Proxy-back** — Cron fires a bounded M2M call to this service's
//!    `POST /v1/internal/schedules/run` with `{schedule_type, env_id,
//!    idempotency_key, correlation_id}`; the handler verifies the bearer
//!    (step 1–4), gates the dedicated action `exchange8:refdata:corporate-
//!    action:transition` through Shield (step 5 — the mirror admits the
//!    cron scope), then runs [`advance_due_transitions`] for that env.
//! 3. **Idempotency** — a transition dedupes on
//!    `(corporate_action id, target stage)`: the guarded `UPDATE ...
//!    WHERE stage = <current and due>` commits exactly once (re-delivery
//!    of a tick finds the stage already advanced and skips).
//! 4. **Emit** — every advanced transition writes (a) the guarded domain
//!    UPDATE, (b) the outbox row (same tx,
//!    `exchange8:refdata:corporate-action:stage-advanced|completed`), and
//!    (c) the audit journal row (`Execute`) in the same transaction.
//!
//! Terminal semantics: `pay_date` is the terminal stage (`Stage::next` is
//! `None`); reaching it emits `completed` (the Settlement handoff key) and
//! ends the row's advancement. `announced → ex_date` adjustments apply at
//! the owning services (Exchange8:Clearing consumes the event; this service does
//! not write positions — design §1 delegation table).

use sqlx::{Acquire, Row};
use uuid::Uuid;

use crate::events::{self, AuditOp, EventIdentity};

/// Cron schedule `domain` (the schedule's owning service slug).
pub const CRON_DOMAIN: &str = "exchange8-refdata";
/// Cron schedule `schedule_type` — Cron's ticker proxies back this kind.
pub const CRON_SCHEDULE_TYPE: &str = "refdata-ca-transitions";
/// Standard 5-field expression: every 5 minutes (low volume, correctness >
/// throughput — master design §9; missed ticks recover on the next tick by
/// re-running the due transitions).
pub const CRON_EXPRESSION: &str = "*/5 * * * *";

/// The Shield action the cron proxy-back must satisfy (step 5). The mirror
/// (registration.rs::SCOPE_MIRROR_POLICIES) admits `sentin8:cron:write`
/// (the proxy-back bearer) plus the platform-admin and CA-write scopes.
pub const TRANSITION_ACTION: &str = "exchange8:refdata:corporate-action:transition";

/// One due transition as scanned from the DB.
#[derive(Debug, Clone, PartialEq)]
pub struct DueTransition {
    /// The corporate-action row id.
    pub action_id: Uuid,
    /// The security the action applies to (envelope snapshot field).
    pub security_id: Uuid,
    /// Current stage key (`announced` / `ex_date` / `record_date`).
    pub current_stage: String,
    /// The next stage key this run advances to.
    pub next_stage: String,
    /// The action kind (`split`, `cash_dividend`, … —
    /// `refdata::ActionType::key`).
    pub action_type: String,
}

/// The stage-vs-date predicate table (design §2 lifecycle):
/// `announced → ex_date` when today ≥ `ex_date`, `… → record_date` when
/// today ≥ `record_date`, `… → pay_date` when today ≥ `pay_date`. Returns
/// the scan SQL's stage ranking inline — kept as a pure function for the
/// stage-order unit test.
pub fn stage_rank(stage: &str) -> Option<u8> {
    match stage {
        "announced" => Some(0),
        "ex_date" => Some(1),
        "record_date" => Some(2),
        "pay_date" => Some(3),
        _ => None,
    }
}

/// Advance EVERY due transition for one env and return the count advanced.
///
/// Each transition is its own transaction (domain UPDATE + outbox row +
/// audit journal row, atomic per action) so one bad row cannot block the
/// batch. Rows already past their due milestone (or terminal) are skipped
/// by the scan; a re-delivered tick is a no-op via the stage-predicated
/// `UPDATE ... WHERE stage = current` guard.
pub async fn advance_due_transitions(
    state: &sentin8::shell::AppState,
    env_id: Uuid,
    subject_id: &str,
    correlation_id: &str,
) -> Result<u32, (axum::http::StatusCode, String)> {
    let today = chrono::Utc::now().date_naive();
    let mut conn = state.pool.acquire().await.map_err(sentin8::shell::err_db)?;

    // Scan: every action whose CURRENT stage's milestone date passed and
    // whose next stage exists. Milestones: announced→ex (ex_date),
    // ex→record (record_date), record→pay (pay_date). The scan is
    // env-scoped by explicit WHERE + RLS context (same-connection rule).
    let mut tx = conn.begin().await.map_err(sentin8::shell::err_db)?;
    sentin8::shell::set_tenant_context(&mut tx, env_id).await;
    let rows = sqlx::query(
        r#"SELECT id, security_id, action_type, stage,
                  ex_date, record_date, pay_date, numerator, denominator
             FROM refdata.corporate_action
            WHERE env_id = $1
              AND (
                    (stage = 'announced'    AND ex_date     IS NOT NULL AND ex_date     <= $2)
                 OR (stage = 'ex_date'      AND record_date IS NOT NULL AND record_date <= $2)
                 OR (stage = 'record_date'  AND pay_date    IS NOT NULL AND pay_date    <= $2)
              )
            ORDER BY ex_date NULLS LAST
            LIMIT 500"#,
    )
    .bind(env_id)
    .bind(today)
    .fetch_all(&mut *tx)
    .await
    .map_err(sentin8::shell::err_db)?;
    let due: Vec<DueTransition> = rows
        .iter()
        .filter_map(|r| {
            let current_stage: String = r.try_get("stage").ok()?;
            let target = next_stage_key(&current_stage)?;
            Some(DueTransition {
                action_id: r.try_get::<Uuid, _>("id").ok()?,
                security_id: r.try_get::<Uuid, _>("security_id").ok()?,
                current_stage,
                next_stage: target.to_string(),
                action_type: r.try_get("action_type").ok()?,
            })
        })
        .collect();
    tx.rollback().await.map_err(sentin8::shell::err_db)?;
    drop(conn);

    let mut advanced = 0u32;
    for t in &due {
        // Per-action transaction: the guarded UPDATE + outbox + journal are
        // atomic; the stage predicate makes a re-delivered tick a no-op.
        let mut conn = state.pool.acquire().await.map_err(sentin8::shell::err_db)?;
        let mut tx = conn.begin().await.map_err(sentin8::shell::err_db)?;
        sentin8::shell::set_tenant_context(&mut tx, env_id).await;
        let payload = sqlx::query(
            r#"UPDATE refdata.corporate_action
                  SET stage = $3
                WHERE env_id = $1 AND id = $2 AND stage = $4
            RETURNING to_jsonb(corporate_action) - 'env_id'"#,
        )
        .bind(env_id)
        .bind(t.action_id)
        .bind(&t.next_stage)
        .bind(&t.current_stage)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sentin8::shell::err_db)?;
        let Some(row) = payload else {
            // Lost the race (already advanced or cancelled) — the
            // idempotency guarantee (action id, target stage).
            tx.rollback().await.map_err(sentin8::shell::err_db)?;
            continue;
        };
        let snapshot: serde_json::Value = row.try_get(0).map_err(sentin8::shell::err_db)?;
        let completed = t.next_stage == "pay_date";
        let identity = EventIdentity {
            env_id,
            subject_id: subject_id.to_string(),
            correlation_id: correlation_id.to_string(),
            idempotency_key: format!("{}:{}", t.action_id, t.next_stage),
        };
        let envelope = events::ca_event(
            if completed {
                events::CaOp::Completed
            } else {
                events::CaOp::StageAdvanced
            },
            &snapshot,
            &identity,
        );
        events::emit_outbox(&mut tx, &identity, &envelope)
            .await
            .map_err(sentin8::shell::err_db)?;
        events::journal_mutation(
            &mut tx,
            &identity,
            AuditOp::Execute,
            "corporate_action",
            &t.action_id.to_string(),
            &format!(
                "corporate action {} transitioned {} → {}",
                t.action_id, t.current_stage, t.next_stage
            ),
            Some(serde_json::json!({ "stage": t.current_stage })),
            Some(serde_json::json!({ "stage": t.next_stage })),
        )
        .await
        .map_err(sentin8::shell::err_db)?;
        tx.commit().await.map_err(sentin8::shell::err_db)?;
        advanced += 1;
    }
    Ok(advanced)
}

/// The next lifecycle stage key after `stage` (None = terminal).
fn next_stage_key(stage: &str) -> Option<&'static str> {
    match stage {
        "announced" => Some("ex_date"),
        "ex_date" => Some("record_date"),
        "record_date" => Some("pay_date"),
        // pay_date is terminal.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_rank_is_strictly_ordered() {
        assert_eq!(stage_rank("announced"), Some(0));
        assert_eq!(stage_rank("ex_date"), Some(1));
        assert_eq!(stage_rank("record_date"), Some(2));
        assert_eq!(stage_rank("pay_date"), Some(3));
        assert_eq!(stage_rank("bogus"), None);
    }

    #[test]
    fn next_stage_walks_the_lifecycle_to_terminal() {
        assert_eq!(next_stage_key("announced"), Some("ex_date"));
        assert_eq!(next_stage_key("ex_date"), Some("record_date"));
        assert_eq!(next_stage_key("record_date"), Some("pay_date"));
        assert_eq!(next_stage_key("pay_date"), None);
        assert_eq!(next_stage_key("garbage"), None);
    }

    #[test]
    fn cron_identifiers_are_stable() {
        // The worker dedupes named schedules via (domain, schedule_type);
        // the identifiers must stay stable across versions.
        assert_eq!(CRON_DOMAIN, "exchange8-refdata");
        assert_eq!(CRON_SCHEDULE_TYPE, "refdata-ca-transitions");
        assert!(CRON_EXPRESSION.split_whitespace().count() == 5);
        // The transition action name follows the event naming format.
        let segs: Vec<&str> = TRANSITION_ACTION.split(':').collect();
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0], "exchange8");
        assert_eq!(segs[1], "refdata");
    }
}
