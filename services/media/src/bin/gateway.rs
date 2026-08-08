use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post, put},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use proofline_media::{
    AppState, CaptureRegistration, Config, build_evidence_artifacts, decode_public_key, emit_event,
    get_object, put_object, register_capture, secret_matches, token_hash,
};
use proofline_protocol::{
    FragmentEnvelope, PROTOCOL_VERSION, ProtocolError, ServerReceipt, SignedReceipt,
    canonical_json, expected_chain_digest, sha256_hex, sign_canonical, verify_canonical,
    verifying_key_from_spki,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tower_http::{
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::info;

#[derive(Debug)]
struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}
impl From<ProtocolError> for ApiError {
    fn from(error: ProtocolError) -> Self {
        Self(StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
    }
}
type ApiResult<T> = Result<T, ApiError>;

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
fn internal(headers: &HeaderMap, state: &AppState) -> bool {
    secret_matches(
        headers
            .get("x-proofline-internal-secret")
            .and_then(|value| value.to_str().ok()),
        &state.config.internal_secret,
    )
}

fn validate_media_container(body: &[u8], declared_mime: &str) -> ApiResult<()> {
    if declared_mime.contains("webm") {
        if body.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
            || body.starts_with(&[0x1f, 0x43, 0xb6, 0x75])
        {
            return Ok(());
        }
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "declared WebM fragment has no EBML header".into(),
        ));
    }
    if !declared_mime.contains("mp4") && !declared_mime.contains("iso.segment") {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "stream MIME is not an accepted evidence container".into(),
        ));
    }
    let mut offset = 0usize;
    let mut boxes = Vec::new();
    while offset + 8 <= body.len() {
        let size32 = u32::from_be_bytes(
            body[offset..offset + 4]
                .try_into()
                .expect("four-byte slice"),
        );
        let kind: [u8; 4] = body[offset + 4..offset + 8]
            .try_into()
            .expect("four-byte slice");
        let (header_size, box_size) = if size32 == 1 {
            if offset + 16 > body.len() {
                return Err(ApiError(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "truncated extended BMFF box".into(),
                ));
            }
            (
                16usize,
                u64::from_be_bytes(
                    body[offset + 8..offset + 16]
                        .try_into()
                        .expect("eight-byte slice"),
                ) as usize,
            )
        } else if size32 == 0 {
            (8usize, body.len() - offset)
        } else {
            (8usize, size32 as usize)
        };
        if box_size < header_size
            || offset
                .checked_add(box_size)
                .is_none_or(|end| end > body.len())
        {
            return Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid BMFF box length".into(),
            ));
        }
        boxes.push(kind);
        offset += box_size;
        if size32 == 0 {
            break;
        }
    }
    if offset != body.len() {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "trailing bytes after BMFF boxes".into(),
        ));
    }
    let has = |name: &[u8; 4]| boxes.iter().any(|kind| kind == name);
    if !has(b"mdat") || !(has(b"moof") || (has(b"ftyp") && has(b"moov"))) {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "BMFF fragment must contain media data and either CMAF movie fragments or a self-contained MP4 header".into()));
    }
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({"status":"ok","service":"proofline-gateway","protocol":PROTOCOL_VERSION}))
}
async fn ready(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({"ready":true})))
}
async fn metrics(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let captures: i64 = sqlx::query_scalar("SELECT count(*) FROM captures")
        .fetch_one(&state.pool)
        .await?;
    let fragments: i64 = sqlx::query_scalar("SELECT count(*) FROM fragments")
        .fetch_one(&state.pool)
        .await?;
    Ok((
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        format!("proofline_captures_total {captures}\nproofline_fragments_total {fragments}\n"),
    )
        .into_response())
}

async fn register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<CaptureRegistration>,
) -> ApiResult<Json<Value>> {
    if !internal(&headers, &state) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid internal secret".into(),
        ));
    }
    if input.streams.is_empty() || input.streams.len() > 3 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "one to three streams are required".into(),
        ));
    }
    register_capture(&state, input).await.map_err(|error| {
        let status = if error.to_string().contains("active capture") {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        ApiError(status, error.to_string())
    })?;
    Ok(Json(json!({"registered":true})))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttestationRequest {
    certificate_chain: Vec<String>,
    challenge: String,
    public_key_spki: String,
}

async fn attest(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<AttestationRequest>,
) -> ApiResult<Json<Value>> {
    if !internal(&headers, &state) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid internal secret".into(),
        ));
    }
    let Some(verifier_url) = &state.config.android_attestation_verifier_url else {
        return Err(ApiError(StatusCode::NOT_IMPLEMENTED, "hardware attestation is fail-closed until PROOFLINE_ANDROID_ATTESTATION_VERIFIER_URL is configured".into()));
    };
    if input.certificate_chain.is_empty()
        || input.challenge.len() > 172
        || input.public_key_spki.is_empty()
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "malformed attestation request".into(),
        ));
    }
    let response = state.client.post(verifier_url).json(&json!({"certificateChain":input.certificate_chain,"challenge":input.challenge,"publicKeySpki":input.public_key_spki})).send().await.map_err(|error| ApiError(StatusCode::BAD_GATEWAY, error.to_string()))?;
    if !response.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "external Android attestation verifier rejected the chain".into(),
        ));
    }
    Ok(Json(response.json().await.map_err(|error| {
        ApiError(StatusCode::BAD_GATEWAY, error.to_string())
    })?))
}

async fn authorize(
    state: &AppState,
    capture_id: &str,
    headers: &HeaderMap,
) -> ApiResult<(String, String)> {
    let token = bearer(headers)
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "missing upload capability".into()))?;
    let row =
        sqlx::query("SELECT upload_token_hash, session_public_key_spki FROM captures WHERE id=$1")
            .bind(capture_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some(row) = row else {
        return Err(ApiError(StatusCode::NOT_FOUND, "capture not found".into()));
    };
    if token_hash(token) != row.get::<String, _>("upload_token_hash") {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid upload capability".into(),
        ));
    }
    Ok((token.to_string(), row.get("session_public_key_spki")))
}

async fn ingest_fragment(
    State(state): State<Arc<AppState>>,
    Path((capture_id, stream_id, sequence)): Path<(String, String, i64)>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<SignedReceipt>> {
    if body.is_empty() || body.len() > state.config.max_fragment_bytes {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "fragment must be between 1 byte and {} bytes",
                state.config.max_fragment_bytes
            ),
        ));
    }
    let (_, public_key_spki) = authorize(&state, &capture_id, &headers).await?;
    let envelope_encoded = headers
        .get("x-proofline-envelope")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "missing fragment envelope".into()))?;
    let signature = headers
        .get("x-proofline-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "missing fragment signature".into()))?;
    let envelope: FragmentEnvelope = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(envelope_encoded)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid envelope encoding".into()))?,
    )
    .map_err(|error| {
        ApiError(
            StatusCode::BAD_REQUEST,
            format!("invalid envelope: {error}"),
        )
    })?;
    if envelope.capture_id != capture_id
        || envelope.stream_id != stream_id
        || envelope.sequence != sequence
        || envelope.protocol_version != PROTOCOL_VERSION
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "route and signed envelope disagree".into(),
        ));
    }
    if envelope.byte_length != body.len() as i64 || envelope.media_digest != sha256_hex(&body) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "media byte hash does not match the signed envelope".into(),
        ));
    }
    if envelope.chain_digest != expected_chain_digest(&envelope)? {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "chain digest is invalid".into(),
        ));
    }
    let key = verifying_key_from_spki(&public_key_spki)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    verify_canonical(&key, &envelope, signature)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;

    let mut transaction = state.pool.begin().await?;
    let stream = sqlx::query("SELECT s.last_sequence,s.last_chain_digest,s.byte_length,s.mime_type,c.status,c.started_at,c.device_fingerprint,c.origin_ip_hash FROM streams s JOIN captures c ON c.id=s.capture_id WHERE s.id=$1 AND s.capture_id=$2 FOR UPDATE").bind(&stream_id).bind(&capture_id).fetch_optional(&mut *transaction).await?;
    let Some(stream) = stream else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "declared stream not found".into(),
        ));
    };
    let last_sequence: i64 = stream.get("last_sequence");
    let last_digest: String = stream.get("last_chain_digest");
    let capture_status: String = stream.get("status");
    if matches!(capture_status.as_str(), "sealed" | "tombstoned") {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!("capture is already {capture_status}"),
        ));
    }
    if envelope.pts_start_us < 0
        || envelope.pts_end_us <= envelope.pts_start_us
        || envelope.pts_end_us > 3_600_000_000
    {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "fragment timestamps are outside the 60-minute capture policy".into(),
        ));
    }
    let mime_type: String = stream.get("mime_type");
    validate_media_container(&body, &mime_type)?;
    let capture_bytes: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(byte_length),0)::bigint FROM streams WHERE capture_id=$1",
    )
    .bind(&capture_id)
    .fetch_one(&mut *transaction)
    .await?;
    if capture_bytes.saturating_add(body.len() as i64) > state.config.max_capture_bytes {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "capture has reached its configured evidence quota".into(),
        ));
    }
    let device_fingerprint: String = stream.get("device_fingerprint");
    let device_daily_bytes: i64 = sqlx::query_scalar("SELECT coalesce(sum(f.byte_length),0)::bigint FROM fragments f JOIN captures c ON c.id=f.capture_id WHERE c.device_fingerprint=$1 AND f.server_received_at >= date_trunc('day',now())")
        .bind(device_fingerprint).fetch_one(&mut *transaction).await?;
    if device_daily_bytes.saturating_add(body.len() as i64) > state.config.max_device_daily_bytes {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "device has reached its configured daily evidence quota".into(),
        ));
    }
    let origin_ip_hash: Option<String> = stream.try_get("origin_ip_hash")?;
    if let Some(origin_ip_hash) = origin_ip_hash {
        let ip_daily_bytes: i64 = sqlx::query_scalar("SELECT coalesce(sum(f.byte_length),0)::bigint FROM fragments f JOIN captures c ON c.id=f.capture_id WHERE c.origin_ip_hash=$1 AND f.server_received_at >= date_trunc('day',now())")
            .bind(origin_ip_hash).fetch_one(&mut *transaction).await?;
        if ip_daily_bytes.saturating_add(body.len() as i64) > state.config.max_ip_daily_bytes {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "origin IP has reached its configured daily evidence quota".into(),
            ));
        }
    }
    if sequence <= last_sequence {
        let existing = sqlx::query("SELECT receipt,receipt_signature FROM fragments WHERE capture_id=$1 AND stream_id=$2 AND sequence=$3 AND media_digest=$4").bind(&capture_id).bind(&stream_id).bind(sequence).bind(&envelope.media_digest).fetch_optional(&mut *transaction).await?;
        if let Some(existing) = existing {
            let receipt: ServerReceipt = serde_json::from_value(existing.get("receipt"))?;
            let signed = SignedReceipt {
                receipt,
                signature: existing.get("receipt_signature"),
                server_public_key_spki: state.server_public_key_spki.clone(),
            };
            return Ok(Json(signed));
        }
        return Err(ApiError(
            StatusCode::CONFLICT,
            "sequence already exists with different bytes".into(),
        ));
    }
    if sequence != last_sequence + 1 || envelope.previous_chain_digest != last_digest {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!(
                "continuity break: expected sequence {} after {}",
                last_sequence + 1,
                last_digest
            ),
        ));
    }

    let object_key = format!("captures/{capture_id}/{stream_id}/{sequence:010}.bin");
    let object_version = put_object(&state, &object_key, body.clone()).await?;
    let received_at = Utc::now();
    let receipt = ServerReceipt {
        protocol_version: PROTOCOL_VERSION.into(),
        capture_id: capture_id.clone(),
        stream_id: stream_id.clone(),
        sequence,
        media_digest: envelope.media_digest.clone(),
        chain_digest: envelope.chain_digest.clone(),
        previous_chain_digest: envelope.previous_chain_digest.clone(),
        byte_length: body.len() as i64,
        server_received_at: received_at.to_rfc3339(),
        object_key: object_key.clone(),
        object_version,
    };
    let receipt_signature = sign_canonical(&state.signer, &receipt)?;
    sqlx::query("INSERT INTO fragments(capture_id,stream_id,sequence,previous_chain_digest,media_digest,chain_digest,byte_length,pts_start_us,pts_end_us,telemetry_root,device_signature,object_key,object_version,server_received_at,receipt,receipt_signature) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(&capture_id).bind(&stream_id).bind(sequence).bind(&envelope.previous_chain_digest).bind(&envelope.media_digest).bind(&envelope.chain_digest).bind(body.len() as i64).bind(envelope.pts_start_us).bind(envelope.pts_end_us).bind(&envelope.telemetry_root).bind(signature).bind(&object_key).bind(&receipt.object_version).bind(received_at).bind(serde_json::to_value(&receipt)?).bind(&receipt_signature).execute(&mut *transaction).await?;
    sqlx::query("UPDATE streams SET last_sequence=$1,last_chain_digest=$2,byte_length=byte_length+$3 WHERE id=$4").bind(sequence).bind(&envelope.chain_digest).bind(body.len() as i64).bind(&stream_id).execute(&mut *transaction).await?;
    sqlx::query("UPDATE captures SET status='live',updated_at=now() WHERE id=$1 AND status IN ('initializing','live','stalled')").bind(&capture_id).execute(&mut *transaction).await?;
    transaction.commit().await?;
    emit_event(&state, &capture_id, "capture.receipt", json!({"stream_id":stream_id,"sequence":sequence,"media_digest":envelope.media_digest,"chain_digest":envelope.chain_digest,"byte_length":body.len(),"server_received_at":received_at})).await?;
    if capture_status == "interrupted" {
        sqlx::query("UPDATE captures SET finalized_at=NULL WHERE id=$1")
            .bind(&capture_id)
            .execute(&state.pool)
            .await?;
        emit_event(&state, &capture_id, "capture.recovery", json!({"stream_id":stream_id,"sequence":sequence,"received_at":received_at,"note":"Valid late continuation retained as a recovery supplement; the original interrupted ending was not rewritten."})).await?;
    }
    Ok(Json(SignedReceipt {
        receipt,
        signature: receipt_signature,
        server_public_key_spki: state.server_public_key_spki.clone(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndRequest {
    manifest: Value,
    signature: String,
}

async fn end_capture(
    State(state): State<Arc<AppState>>,
    Path(capture_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<EndRequest>,
) -> ApiResult<Json<Value>> {
    authorize(&state, &capture_id, &headers).await?;
    let device_public_key_spki: String = sqlx::query_scalar(
        "SELECT coalesce(device_public_key_spki,session_public_key_spki) FROM captures WHERE id=$1",
    )
    .bind(&capture_id)
    .fetch_one(&state.pool)
    .await?;
    let key = verifying_key_from_spki(&device_public_key_spki)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    verify_canonical(&key, &input.manifest, &input.signature)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    if input.manifest.get("captureId").and_then(Value::as_str) != Some(&capture_id) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "manifest capture id is invalid".into(),
        ));
    }
    if input
        .manifest
        .get("protocolVersion")
        .and_then(Value::as_str)
        != Some(PROTOCOL_VERSION)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "manifest protocol version is invalid".into(),
        ));
    }
    let streams = input
        .manifest
        .get("streams")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "manifest streams are missing".into(),
            )
        })?;
    let declared: Vec<String> =
        sqlx::query_scalar("SELECT id FROM streams WHERE capture_id=$1 ORDER BY id")
            .bind(&capture_id)
            .fetch_all(&state.pool)
            .await?;
    let mut manifest_ids: Vec<String> = streams
        .iter()
        .filter_map(|stream| stream.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    manifest_ids.sort();
    manifest_ids.dedup();
    let mut complete = manifest_ids == declared;
    for stream in streams {
        let id = stream.get("id").and_then(Value::as_str).unwrap_or_default();
        let count = stream
            .get("sequenceCount")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let digest = stream
            .get("finalChainDigest")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let row = sqlx::query(
            "SELECT last_sequence,last_chain_digest FROM streams WHERE capture_id=$1 AND id=$2",
        )
        .bind(&capture_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
        complete &= row.as_ref().is_some_and(|row| {
            row.get::<i64, _>("last_sequence") + 1 == count
                && row.get::<String, _>("last_chain_digest") == digest
        });
    }
    let ended_at = input
        .manifest
        .get("endedAt")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);
    let duration_ms = input
        .manifest
        .get("durationMs")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let close_reason = input
        .manifest
        .get("closeReason")
        .and_then(Value::as_str)
        .unwrap_or("user_stop");
    if !(0..=3_600_000).contains(&duration_ms) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "manifest duration exceeds the 60-minute policy".into(),
        ));
    }
    if !matches!(
        close_reason,
        "user_stop" | "duration_limit" | "permission_revoked" | "thermal_shutdown" | "app_error"
    ) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "manifest close reason is not recognized".into(),
        ));
    }
    let current_status: String = sqlx::query_scalar("SELECT status FROM captures WHERE id=$1")
        .bind(&capture_id)
        .fetch_one(&state.pool)
        .await?;
    if matches!(current_status.as_str(), "sealed" | "tombstoned") {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!("capture is already {current_status}"),
        ));
    }
    let completeness = if complete {
        "complete_with_signed_end"
    } else {
        "gaps_detected"
    };
    sqlx::query("UPDATE captures SET status='sealed',completeness=$2,ended_at=$3,close_reason=$4,final_manifest=$5,final_signature=$6,updated_at=now() WHERE id=$1").bind(&capture_id).bind(completeness).bind(ended_at).bind(close_reason).bind(&input.manifest).bind(&input.signature).execute(&state.pool).await?;
    let verification = json!({"fragmentChain":if complete{"pass"}else{"fail"},"deviceSignature":"pass","audioBinding":"pending","serverReceipts":"pass","timestampAnchor":"pending","c2pa":"unsupported"});
    emit_event(&state, &capture_id, "capture.sealed", json!({"completeness":completeness,"ended_at":ended_at,"duration_ms":duration_ms,"close_reason":close_reason,"verification":verification})).await?;
    build_evidence_artifacts(&state, &capture_id).await?;
    Ok(Json(
        json!({"status":"sealed","completeness":completeness,"serverPublicKeySpki":state.server_public_key_spki}),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryRequest {
    sequence: i64,
    previous_digest: String,
    digest: String,
    signature: String,
    samples: Value,
}
async fn telemetry(
    State(state): State<Arc<AppState>>,
    Path(capture_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<TelemetryRequest>,
) -> ApiResult<Json<Value>> {
    let (_, public_key_spki) = authorize(&state, &capture_id, &headers).await?;
    let signed = json!({"captureId":capture_id,"sequence":input.sequence,"previousDigest":input.previous_digest,"digest":input.digest,"samples":input.samples});
    let key = verifying_key_from_spki(&public_key_spki)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    verify_canonical(&key, &signed, &input.signature)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    let expected = sha256_hex(canonical_json(&json!({"captureId":capture_id,"sequence":input.sequence,"previousDigest":input.previous_digest,"samples":input.samples}))?.as_bytes());
    if expected != input.digest {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "telemetry digest is invalid".into(),
        ));
    }
    if serde_json::to_vec(&input.samples)?.len() > 2 * 1024 * 1024 {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "telemetry batch exceeds 2 MiB".into(),
        ));
    }
    if input.sequence < 0 || input.digest.len() != 64 || input.previous_digest.len() != 64 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "telemetry sequence or digest is malformed".into(),
        ));
    }
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query("SELECT sequence,digest FROM telemetry_batches WHERE capture_id=$1 ORDER BY sequence DESC LIMIT 1 FOR UPDATE").bind(&capture_id).fetch_optional(&mut *transaction).await?;
    if let Some(previous) = previous {
        let previous_sequence: i64 = previous.get("sequence");
        let previous_digest: String = previous.get("digest");
        if input.sequence <= previous_sequence {
            let same: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM telemetry_batches WHERE capture_id=$1 AND sequence=$2 AND digest=$3)").bind(&capture_id).bind(input.sequence).bind(&input.digest).fetch_one(&mut *transaction).await?;
            if same {
                return Ok(Json(json!({"accepted":true,"duplicate":true})));
            }
            return Err(ApiError(
                StatusCode::CONFLICT,
                "telemetry sequence conflicts with retained evidence".into(),
            ));
        }
        if input.sequence != previous_sequence + 1 || input.previous_digest != previous_digest {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "telemetry continuity break".into(),
            ));
        }
    } else if input.sequence != 0 || input.previous_digest != proofline_protocol::GENESIS_DIGEST {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "first telemetry batch must begin at the genesis digest".into(),
        ));
    }
    sqlx::query("INSERT INTO telemetry_batches(capture_id,sequence,digest,previous_digest,device_signature,payload) VALUES($1,$2,$3,$4,$5,$6)").bind(&capture_id).bind(input.sequence).bind(input.digest).bind(input.previous_digest).bind(input.signature).bind(input.samples).execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(Json(json!({"accepted":true})))
}

async fn live_playlist(
    State(state): State<Arc<AppState>>,
    Path((capture_id, stream_id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let fragments = sqlx::query("SELECT sequence,pts_start_us,pts_end_us FROM fragments WHERE capture_id=$1 AND stream_id=$2 ORDER BY sequence").bind(&capture_id).bind(&stream_id).fetch_all(&state.pool).await?;
    if fragments.is_empty() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "no fragments received".into(),
        ));
    }
    let status: String = sqlx::query_scalar("SELECT status FROM captures WHERE id=$1")
        .bind(&capture_id)
        .fetch_one(&state.pool)
        .await?;
    if status == "tombstoned" {
        return Err(ApiError(
            StatusCode::GONE,
            "media is tombstoned; audit metadata remains available".into(),
        ));
    }
    let mut playlist = String::from(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:3\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-INDEPENDENT-SEGMENTS\n",
    );
    for row in fragments {
        let sequence: i64 = row.get("sequence");
        let duration = (row.get::<i64, _>("pts_end_us") - row.get::<i64, _>("pts_start_us")).max(1)
            as f64
            / 1_000_000.0;
        playlist.push_str(&format!(
            "#EXTINF:{duration:.3},\n/live/v1/{capture_id}/{stream_id}/fragment/{sequence}\n"
        ));
    }
    if matches!(status.as_str(), "sealed" | "interrupted" | "tombstoned") {
        playlist.push_str("#EXT-X-ENDLIST\n");
    }
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        playlist,
    )
        .into_response())
}

async fn live_fragment(
    State(state): State<Arc<AppState>>,
    Path((capture_id, stream_id, sequence)): Path<(String, String, i64)>,
) -> ApiResult<Response> {
    let row = sqlx::query(
        "SELECT f.object_key,c.status FROM fragments f JOIN captures c ON c.id=f.capture_id WHERE f.capture_id=$1 AND f.stream_id=$2 AND f.sequence=$3",
    )
    .bind(capture_id)
    .bind(stream_id)
    .bind(sequence)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(ApiError(StatusCode::NOT_FOUND, "fragment not found".into()));
    };
    if row.get::<String, _>("status") == "tombstoned" {
        return Err(ApiError(
            StatusCode::GONE,
            "media is tombstoned; audit metadata remains available".into(),
        ));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "video/iso.segment"),
            (header::CACHE_CONTROL, "public,max-age=31536000,immutable"),
        ],
        get_object(&state, row.get("object_key")).await?,
    )
        .into_response())
}

async fn original_stream(
    State(state): State<Arc<AppState>>,
    Path((capture_id, stream_id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let tombstoned: bool =
        sqlx::query_scalar("SELECT status='tombstoned' FROM captures WHERE id=$1")
            .bind(&capture_id)
            .fetch_one(&state.pool)
            .await?;
    if tombstoned {
        return Err(ApiError(
            StatusCode::GONE,
            "media is tombstoned; audit metadata remains available".into(),
        ));
    }
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT object_key FROM fragments WHERE capture_id=$1 AND stream_id=$2 ORDER BY sequence",
    )
    .bind(&capture_id)
    .bind(&stream_id)
    .fetch_all(&state.pool)
    .await?;
    if keys.is_empty() {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream not found".into()));
    }
    let objects = state.objects.clone();
    let stream = futures::stream::iter(keys).then(move |key| {
        let objects = objects.clone();
        async move {
            let object = objects
                .get(&object_store::path::Path::from(key))
                .await
                .map_err(std::io::Error::other)?;
            object.bytes().await.map_err(std::io::Error::other)
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{capture_id}-{stream_id}.bin\""
        ))
        .unwrap(),
    );
    Ok(response)
}

async fn object_download(
    state: Arc<AppState>,
    capture_id: String,
    name: &str,
    content_type: &'static str,
) -> ApiResult<Response> {
    let key = format!("evidence/{capture_id}/{name}");
    let bytes = get_object(&state, &key).await.map_err(|_| {
        ApiError(
            StatusCode::NOT_FOUND,
            "evidence artifact is not ready".into(),
        )
    })?;
    let disposition = match name {
        "report.pdf" => "attachment; filename=proofline-report.pdf",
        "report.json" => "attachment; filename=proofline-evidence.json",
        _ => "attachment; filename=proofline-evidence.zip",
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response())
}
async fn report_pdf(
    State(state): State<Arc<AppState>>,
    Path(capture_id): Path<String>,
) -> ApiResult<Response> {
    object_download(state, capture_id, "report.pdf", "application/pdf").await
}
async fn report_json(
    State(state): State<Arc<AppState>>,
    Path(capture_id): Path<String>,
) -> ApiResult<Response> {
    object_download(state, capture_id, "report.json", "application/json").await
}
async fn evidence_bundle(
    State(state): State<Arc<AppState>>,
    Path(capture_id): Path<String>,
) -> ApiResult<Response> {
    let tombstoned: bool =
        sqlx::query_scalar("SELECT status='tombstoned' FROM captures WHERE id=$1")
            .bind(&capture_id)
            .fetch_one(&state.pool)
            .await?;
    if tombstoned {
        return Err(ApiError(StatusCode::GONE, "raw evidence bundle is hidden by the public tombstone; report metadata remains available".into()));
    }
    object_download(state, capture_id, "bundle.zip", "application/zip").await
}

async fn events(
    State(state): State<Arc<AppState>>,
    Path(capture_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! { let mut timer = tokio::time::interval(Duration::from_secs(2)); loop { timer.tick().await; let row = sqlx::query("SELECT c.status, max(f.server_received_at) AS last_receipt, count(f.sequence) AS fragments, coalesce(sum(f.byte_length),0)::bigint AS bytes FROM captures c LEFT JOIN fragments f ON f.capture_id=c.id WHERE c.id=$1 GROUP BY c.status").bind(&capture_id).fetch_optional(&state.pool).await; let payload = match row { Ok(Some(row)) => json!({"status":row.get::<String,_>("status"),"lastReceipt":row.try_get::<Option<DateTime<Utc>>,_>("last_receipt").ok().flatten(),"fragments":row.get::<i64,_>("fragments"),"receivedBytes":row.get::<i64,_>("bytes")}), _ => json!({"status":"not_found"}) }; yield Ok(Event::default().json_data(payload).unwrap()); } };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TombstoneAction {
    capture_id: String,
    reason: String,
    issued_at: String,
    nonce: String,
}
async fn tombstone(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(action): Json<TombstoneAction>,
) -> ApiResult<Json<Value>> {
    if !internal(&headers, &state) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid internal secret".into(),
        ));
    }
    let public = state
        .config
        .admin_public_key_spki
        .as_deref()
        .ok_or_else(|| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin public key is not configured".into(),
            )
        })?;
    let signature = headers
        .get("x-proofline-admin-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "missing admin signature".into()))?;
    let key = decode_public_key(public)?;
    verify_canonical(&key, &action, signature)
        .map_err(|error| ApiError(StatusCode::UNAUTHORIZED, error.to_string()))?;
    if (Utc::now()
        - action
            .issued_at
            .parse::<DateTime<Utc>>()
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid issuedAt".into()))?)
    .num_minutes()
    .abs()
        > 10
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "tombstone action is outside its validity window".into(),
        ));
    }
    let value = serde_json::to_value(&action)?;
    sqlx::query("INSERT INTO tombstones(capture_id,reason,action_json,admin_signature) VALUES($1,$2,$3,$4) ON CONFLICT(capture_id) DO NOTHING").bind(&action.capture_id).bind(&action.reason).bind(value).bind(signature).execute(&state.pool).await?;
    sqlx::query(
        "UPDATE captures SET status='tombstoned',tombstone_reason=$2,updated_at=now() WHERE id=$1",
    )
    .bind(&action.capture_id)
    .bind(&action.reason)
    .execute(&state.pool)
    .await?;
    emit_event(
        &state,
        &action.capture_id,
        "capture.tombstoned",
        json!({"reason":action.reason}),
    )
    .await?;
    build_evidence_artifacts(&state, &action.capture_id).await?;
    Ok(Json(json!({"tombstoned":true})))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    let bind = config.bind.clone();
    let state = Arc::new(AppState::initialize(config).await?);
    let request_id = header::HeaderName::from_static("x-request-id");
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/internal/v1/captures", post(register))
        .route("/internal/v1/attest", post(attest))
        .route("/internal/v1/tombstones", post(tombstone))
        .route(
            "/ingest/v1/{capture_id}/{stream_id}/{sequence}",
            put(ingest_fragment),
        )
        .route("/ingest/v1/{capture_id}/telemetry", post(telemetry))
        .route("/ingest/v1/{capture_id}/end", post(end_capture))
        .route(
            "/live/v1/{capture_id}/{stream_id}/index.m3u8",
            get(live_playlist),
        )
        .route(
            "/live/v1/{capture_id}/{stream_id}/fragment/{sequence}",
            get(live_fragment),
        )
        .route("/events/v1/{capture_id}", get(events))
        .route(
            "/evidence/v1/{capture_id}/original/{stream_id}",
            get(original_stream),
        )
        .route("/evidence/v1/{capture_id}/report.pdf", get(report_pdf))
        .route("/evidence/v1/{capture_id}/report.json", get(report_json))
        .route("/evidence/v1/{capture_id}/bundle.zip", get(evidence_bundle))
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "ProofLine media gateway ready");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
