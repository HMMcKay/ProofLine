use bytes::Bytes;
use chrono::{DateTime, Utc};
use proofline_media::{
    AppState, Config, PublicLedgerEvent, build_evidence_artifacts, deliver_event, emit_event,
    merkle_root_hex, put_object, rfc3161_sha256_request,
};
use proofline_protocol::{canonical_json, sign_canonical};
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::postgres::PgRow;
use std::{sync::Arc, time::Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

async fn anchor_receipts(state: &AppState) -> anyhow::Result<()> {
    let mut transaction = state.pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(73466201)")
        .fetch_one(&mut *transaction)
        .await?;
    if !locked {
        return Ok(());
    }
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT f.capture_id,f.stream_id,f.sequence,f.receipt \
         FROM fragments f JOIN captures c ON c.id=f.capture_id \
         WHERE f.anchor_id IS NULL \
           AND (f.server_received_at < now() - interval '60 seconds' OR c.status IN ('sealed','interrupted')) \
         ORDER BY f.server_received_at,f.capture_id,f.stream_id,f.sequence LIMIT 10000 FOR UPDATE OF f SKIP LOCKED",
    )
    .fetch_all(&mut *transaction)
    .await?;
    if rows.is_empty() {
        transaction.rollback().await?;
        return Ok(());
    }

    let leaves: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "capture_id": row.get::<String, _>("capture_id"),
                "stream_id": row.get::<String, _>("stream_id"),
                "sequence": row.get::<i64, _>("sequence"),
                "receipt": row.get::<Value, _>("receipt")
            })
        })
        .collect();
    let leaf_bytes = leaves
        .iter()
        .map(|leaf| canonical_json(leaf).map(String::into_bytes))
        .collect::<Result<Vec<_>, _>>()?;
    let root = merkle_root_hex(&leaf_bytes)?;
    let anchor_id = Uuid::new_v4();
    let created_at = Utc::now();
    let leaf_set = Value::Array(leaves.clone());
    let signed = json!({
        "protocol_version":"proofline-evidence/2.0",
        "anchor_id":anchor_id,
        "created_at":created_at,
        "merkle_root":root,
        "leaf_count":leaves.len(),
        "leaf_set":&leaf_set
    });
    let signature = sign_canonical(&state.signer, &signed)?;
    let tsa_status = if state.config.tsa_url.is_some() {
        "pending"
    } else {
        "not_configured"
    };
    sqlx::query("INSERT INTO receipt_anchors(id,created_at,merkle_root,leaf_count,leaf_set,server_signature,tsa_url,tsa_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(anchor_id).bind(created_at).bind(&root).bind(leaves.len() as i32).bind(&leaf_set)
        .bind(&signature).bind(&state.config.tsa_url).bind(tsa_status).execute(&mut *transaction).await?;
    for row in &rows {
        sqlx::query("UPDATE fragments SET anchor_id=$1 WHERE capture_id=$2 AND stream_id=$3 AND sequence=$4 AND anchor_id IS NULL")
            .bind(anchor_id).bind(row.get::<String,_>("capture_id")).bind(row.get::<String,_>("stream_id"))
            .bind(row.get::<i64,_>("sequence")).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;

    let tsa_result = if let Some(url) = &state.config.tsa_url {
        let request = rfc3161_sha256_request(&root)?;
        match state
            .client
            .post(url)
            .header("content-type", "application/timestamp-query")
            .header("accept", "application/timestamp-reply")
            .body(request)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                let body = response.bytes().await?;
                if body.is_empty() {
                    Err(anyhow::anyhow!("TSA returned an empty success response"))
                } else {
                    let key = format!("anchors/{anchor_id}/response.tsr");
                    put_object(state, &key, Bytes::from(body.to_vec())).await?;
                    sqlx::query("UPDATE receipt_anchors SET tsa_status='received_unvalidated',tsa_response_object_key=$2,tsa_error=NULL WHERE id=$1")
                        .bind(anchor_id).bind(&key).execute(&state.pool).await?;
                    Ok(())
                }
            }
            Ok(response) => Err(anyhow::anyhow!("TSA returned HTTP {}", response.status())),
            Err(error) => Err(error.into()),
        }
    } else {
        Ok(())
    };
    if let Err(error) = tsa_result {
        let message = error.to_string();
        sqlx::query("UPDATE receipt_anchors SET tsa_status='failed',tsa_error=$2 WHERE id=$1")
            .bind(anchor_id)
            .bind(&message)
            .execute(&state.pool)
            .await?;
        warn!(%anchor_id,%error,"timestamp authority request failed; durable receipts remain valid");
    }

    let captures: Vec<String> = rows.iter().map(|row| row.get("capture_id")).collect();
    let mut captures = captures;
    captures.sort();
    captures.dedup();
    sqlx::query("UPDATE captures SET finalized_at=NULL WHERE id = ANY($1) AND status IN ('sealed','interrupted')")
        .bind(&captures)
        .execute(&state.pool)
        .await?;
    for capture_id in captures {
        emit_event(state, &capture_id, "receipt.anchor", json!({
            "anchor_id":anchor_id,"merkle_root":root,"leaf_count":leaves.len(),"tsa_status":if state.config.tsa_url.is_some() {"requested"} else {"not_configured"}
        })).await?;
    }
    info!(%anchor_id,leaf_count=leaves.len(),"receipt Merkle anchor created");
    Ok(())
}

async fn update_liveness(state: &AppState) -> anyhow::Result<()> {
    let stalled: Vec<String> = sqlx::query_scalar("UPDATE captures c SET status='stalled',updated_at=now() WHERE c.status='live' AND coalesce((SELECT max(f.server_received_at) FROM fragments f WHERE f.capture_id=c.id),c.started_at) < now() - interval '30 seconds' RETURNING c.id").fetch_all(&state.pool).await?;
    for id in stalled {
        emit_event(
            state,
            &id,
            "capture.stalled",
            json!({"completeness":"pending"}),
        )
        .await?;
    }
    let interrupted: Vec<String> = sqlx::query_scalar("UPDATE captures c SET status='interrupted',completeness='complete_as_server_received',ended_at=coalesce((SELECT max(f.server_received_at) FROM fragments f WHERE f.capture_id=c.id),c.started_at),close_reason='server_timeout',updated_at=now() WHERE c.status IN ('initializing','live','stalled') AND coalesce((SELECT max(f.server_received_at) FROM fragments f WHERE f.capture_id=c.id),c.started_at) < now() - interval '15 minutes' RETURNING c.id").fetch_all(&state.pool).await?;
    for id in interrupted {
        let duration_ms: i64 = sqlx::query_scalar("SELECT greatest(0,extract(epoch from (coalesce(ended_at,now())-started_at))*1000)::bigint FROM captures WHERE id=$1").bind(&id).fetch_one(&state.pool).await?;
        emit_event(state, &id, "capture.interrupted", json!({"completeness":"complete_as_server_received","ended_at":Utc::now(),"duration_ms":duration_ms,"close_reason":"server_timeout","verification":{"fragmentChain":"pass","deviceSignature":"warn","audioBinding":"pending","serverReceipts":"pass","timestampAnchor":"pending","c2pa":"unsupported"}})).await?;
    }
    Ok(())
}

async fn finalize_ready(state: &AppState) -> anyhow::Result<()> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM captures WHERE status IN ('sealed','interrupted') AND finalized_at IS NULL ORDER BY updated_at LIMIT 4").fetch_all(&state.pool).await?;
    for id in ids {
        if let Err(error) = build_evidence_artifacts(state, &id).await {
            error!(capture_id=%id,%error,"artifact finalization failed");
        }
    }
    Ok(())
}

async fn deliver_outbox(state: &AppState) -> anyhow::Result<()> {
    let rows = sqlx::query("SELECT id,capture_id,event_type,occurred_at,payload,server_signature FROM evidence_events WHERE delivered_at IS NULL ORDER BY occurred_at LIMIT 50").fetch_all(&state.pool).await?;
    for row in rows {
        let id: Uuid = row.get("id");
        let event = PublicLedgerEvent {
            id: id.to_string(),
            capture_id: row.get("capture_id"),
            event_type: row.get("event_type"),
            occurred_at: row.get::<DateTime<Utc>, _>("occurred_at").to_rfc3339(),
            payload: row.get("payload"),
            signature: row.get("server_signature"),
        };
        if let Err(error) = deliver_event(state, &event).await {
            warn!(event_id=%id,%error,"outbox delivery failed");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let state = Arc::new(AppState::initialize(Config::from_env()?).await?);
    info!("ProofLine evidence worker ready");
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = timer.tick() => {
                if let Err(error) = update_liveness(&state).await { error!(%error,"liveness update failed"); }
                if let Err(error) = anchor_receipts(&state).await { error!(%error,"receipt anchor pass failed"); }
                if let Err(error) = finalize_ready(&state).await { error!(%error,"finalizer pass failed"); }
                if let Err(error) = deliver_outbox(&state).await { error!(%error,"outbox pass failed"); }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}
