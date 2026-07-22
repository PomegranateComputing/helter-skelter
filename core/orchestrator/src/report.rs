//! `orchestrator report --simulation <id>` -- a Markdown operator report
//! for one simulation, per the milestone 0.1 acceptance run's
//! requirements: a safety-state timeline, the full decision ledger,
//! incidents, final KPIs, and unexplained anomalies. Read-only; nothing
//! here writes to the database.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db;
use crate::error::OrchestratorError;

pub async fn generate(pool: &PgPool, simulation_id: Uuid) -> Result<String, OrchestratorError> {
    let simulation = db::find_simulation(pool, simulation_id)
        .await?
        .ok_or(OrchestratorError::SimulationNotFound(simulation_id))?;
    let transitions = db::state_transitions_for_report(pool, simulation_id).await?;
    let ledger = db::ledger_for_report(pool, simulation_id).await?;
    let rollbacks = db::rollbacks_for_report(pool, simulation_id).await?;
    let kpis = db::final_kpis(pool, simulation_id).await?;
    let in_flight = db::in_flight_actions_for_simulation(pool, simulation_id).await?;

    let mut out = String::new();
    let now = Utc::now();

    out.push_str(&format!(
        "# Operator report: simulation {simulation_id}\n\n"
    ));
    out.push_str(&format!("Generated: {}\n\n", fmt_ts(now)));
    out.push_str("## Simulation\n\n");
    out.push_str(&format!("- Started: {}\n", fmt_ts(simulation.started_at)));
    out.push_str(&format!(
        "- Ended: {}\n",
        simulation
            .ended_at
            .map(fmt_ts)
            .unwrap_or_else(|| "(still running)".to_string())
    ));
    out.push_str(&format!(
        "- Bridge version: {}\n",
        simulation.bridge_version
    ));
    out.push_str(&format!(
        "- OpenRCT2 version: {}\n\n",
        simulation.openrct2_version
    ));

    out.push_str("## Safety-state timeline\n\n");
    if transitions.is_empty() {
        out.push_str("_No state transitions recorded._\n\n");
    } else {
        out.push_str("| time | from | to | triggered by | reason |\n");
        out.push_str("|---|---|---|---|---|\n");
        for t in &transitions {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                fmt_ts(t.created_at),
                t.from_state,
                t.to_state,
                t.triggered_by,
                escape_pipes(&t.reason)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Decision ledger (proposal -> authorization -> action -> result)\n\n");
    if ledger.is_empty() {
        out.push_str("_No proposals were made during this simulation._\n\n");
    } else {
        out.push_str("| time | agent | confidence | decision | reason | action | engine_cost | engine_error |\n");
        out.push_str("|---|---|---|---|---|---|---|---|\n");
        for row in &ledger {
            out.push_str(&format!(
                "| {} | {} | {:.2} | {} | {} | {} | {} | {} |\n",
                fmt_ts(row.proposal_created_at),
                row.agent,
                row.confidence,
                row.decision
                    .as_deref()
                    .unwrap_or("(no authorization recorded)"),
                row.auth_reason
                    .as_deref()
                    .map(escape_pipes)
                    .unwrap_or_default(),
                row.command
                    .as_ref()
                    .map(|c| escape_pipes(&c.to_string()))
                    .unwrap_or_else(|| "-".to_string()),
                row.engine_cost
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                row.engine_error
                    .as_ref()
                    .map(|e| escape_pipes(&e.to_string()))
                    .unwrap_or_else(|| "-".to_string()),
            ));
        }
        out.push('\n');
    }

    out.push_str("## Prediction errors and incidents\n\n");
    let engine_error_count = ledger.iter().filter(|r| r.engine_error.is_some()).count();
    out.push_str(&format!(
        "- Actions with an engine_error: {engine_error_count}\n"
    ));
    out.push_str(&format!("- Rollbacks recorded: {}\n\n", rollbacks.len()));
    if rollbacks.is_empty() {
        out.push_str("_No rollbacks occurred._\n\n");
    } else {
        out.push_str("| time | snapshot | triggered by | reason |\n");
        out.push_str("|---|---|---|---|\n");
        for r in &rollbacks {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                fmt_ts(r.created_at),
                r.snapshot_id,
                r.triggered_by,
                escape_pipes(&r.reason)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Final KPIs\n\n");
    match kpis {
        Some(k) => {
            out.push_str(&format!("- Cash: {}\n", k.cash));
            out.push_str(&format!("- Guest count: {}\n", k.guest_count));
            out.push_str(&format!("- Park rating: {}\n", k.park_rating));
            out.push_str(&format!(
                "- As of observation recorded at: {}\n\n",
                fmt_ts(k.recorded_at)
            ));
        }
        None => out.push_str("_No observations were recorded for this simulation._\n\n"),
    }

    out.push_str("## Unexplained anomalies\n\n");
    if in_flight.is_empty() {
        out.push_str("_None: every authorized action has a recorded result._\n\n");
    } else {
        out.push_str(&format!(
            "- {} action(s) with no recorded result (idempotency_key(s): {}):\n\n",
            in_flight.len(),
            in_flight
                .iter()
                .map(|a| a.idempotency_key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(out)
}

fn fmt_ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn escape_pipes(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}
