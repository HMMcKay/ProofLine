use anyhow::{Context, Result, anyhow};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use object_store::{ObjectStore, aws::AmazonS3Builder, path::Path as ObjectPath};
use p256::ecdsa::{SigningKey, VerifyingKey};
use p256::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey};
use proofline_protocol::{
    PROTOCOL_VERSION, canonical_json, public_key_spki, sha256_hex, sign_canonical,
};
use qrcodegen::{QrCode, QrCodeEcc};
use rand_core::OsRng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    env,
    io::{Cursor, Write},
    sync::Arc,
};
use subtle::ConstantTimeEq;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub bucket: String,
    pub object_endpoint: Option<String>,
    pub object_region: String,
    pub object_access_key: Option<String>,
    pub object_secret_key: Option<String>,
    pub internal_secret: String,
    pub ledger_event_url: Option<String>,
    pub android_attestation_verifier_url: Option<String>,
    pub admin_public_key_spki: Option<String>,
    pub tsa_url: Option<String>,
    pub max_fragment_bytes: usize,
    pub max_capture_bytes: i64,
    pub max_device_daily_bytes: i64,
    pub max_ip_daily_bytes: i64,
    pub max_device_concurrent: i64,
    pub max_ip_concurrent: i64,
    pub public_web_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            bind: env::var("PROOFLINE_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            bucket: env::var("PROOFLINE_BUCKET").unwrap_or_else(|_| "proofline-media".into()),
            object_endpoint: optional_env("PROOFLINE_OBJECT_ENDPOINT"),
            object_region: env::var("PROOFLINE_OBJECT_REGION")
                .unwrap_or_else(|_| "us-east-1".into()),
            object_access_key: optional_env("PROOFLINE_OBJECT_ACCESS_KEY"),
            object_secret_key: optional_env("PROOFLINE_OBJECT_SECRET_KEY"),
            internal_secret: env::var("PROOFLINE_INTERNAL_SECRET")
                .context("PROOFLINE_INTERNAL_SECRET is required")?,
            ledger_event_url: optional_env("PROOFLINE_LEDGER_EVENT_URL"),
            android_attestation_verifier_url: optional_env(
                "PROOFLINE_ANDROID_ATTESTATION_VERIFIER_URL",
            ),
            admin_public_key_spki: optional_env("PROOFLINE_ADMIN_PUBLIC_KEY_SPKI"),
            tsa_url: optional_env("PROOFLINE_TSA_URL"),
            max_fragment_bytes: numeric_env("PROOFLINE_MAX_FRAGMENT_BYTES", 32 * 1024 * 1024)?,
            max_capture_bytes: numeric_env(
                "PROOFLINE_MAX_CAPTURE_BYTES",
                5 * 1024 * 1024 * 1024_i64,
            )?,
            max_device_daily_bytes: numeric_env(
                "PROOFLINE_MAX_DEVICE_DAILY_BYTES",
                10 * 1024 * 1024 * 1024_i64,
            )?,
            max_ip_daily_bytes: numeric_env(
                "PROOFLINE_MAX_IP_DAILY_BYTES",
                25 * 1024 * 1024 * 1024_i64,
            )?,
            max_device_concurrent: numeric_env("PROOFLINE_MAX_DEVICE_CONCURRENT", 1_i64)?,
            max_ip_concurrent: numeric_env("PROOFLINE_MAX_IP_CONCURRENT", 3_i64)?,
            public_web_url: optional_env("PROOFLINE_PUBLIC_WEB_URL"),
        })
    }
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn numeric_env<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match optional_env(name) {
        Some(value) => value
            .parse()
            .map_err(|error| anyhow!("invalid {name}: {error}")),
        None => Ok(default),
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub objects: Arc<dyn ObjectStore>,
    pub signer: Arc<SigningKey>,
    pub server_public_key_spki: String,
    pub client: Client,
}

impl AppState {
    pub async fn initialize(config: Config) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&config.database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let objects = object_store(&config)?;
        let signer = Arc::new(load_server_signing_key()?);
        let server_public_key_spki = public_key_spki(signer.verifying_key())?;
        Ok(Self {
            config,
            pool,
            objects,
            signer,
            server_public_key_spki,
            client: Client::new(),
        })
    }
}

fn object_store(config: &Config) -> Result<Arc<dyn ObjectStore>> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket)
        .with_region(&config.object_region);
    if let Some(endpoint) = &config.object_endpoint {
        builder = builder
            .with_endpoint(endpoint)
            .with_allow_http(endpoint.starts_with("http://"));
    }
    if let Some(access_key) = &config.object_access_key {
        builder = builder.with_access_key_id(access_key);
    }
    if let Some(secret_key) = &config.object_secret_key {
        builder = builder.with_secret_access_key(secret_key);
    }
    Ok(Arc::new(builder.build()?))
}

fn load_server_signing_key() -> Result<SigningKey> {
    if let Some(encoded) = optional_env("PROOFLINE_SERVER_SIGNING_KEY_B64") {
        let der = URL_SAFE_NO_PAD.decode(encoded)?;
        return SigningKey::from_pkcs8_der(&der)
            .map_err(|error| anyhow!("invalid server signing key: {error}"));
    }
    if env::var("PROOFLINE_ALLOW_EPHEMERAL_SIGNING_KEY").as_deref() != Ok("true") {
        return Err(anyhow!(
            "PROOFLINE_SERVER_SIGNING_KEY_B64 is required unless the explicit development-only ephemeral-key flag is true"
        ));
    }
    warn!("using an ephemeral development receipt key; signatures will not survive restart");
    Ok(SigningKey::random(&mut OsRng))
}

pub fn secret_matches(provided: Option<&str>, expected: &str) -> bool {
    let Some(provided) = provided else {
        return false;
    };
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StreamRegistration {
    pub id: String,
    pub role: String,
    pub mime_type: String,
    pub codec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CaptureRegistration {
    pub capture_id: String,
    pub device_fingerprint: String,
    pub assurance_level: String,
    pub session_public_key_spki: String,
    pub device_public_key_spki: Option<String>,
    pub session_binding_signature: String,
    pub upload_token: String,
    pub started_at: DateTime<Utc>,
    pub origin_ip_hash: Option<String>,
    pub streams: Vec<StreamRegistration>,
}

pub async fn register_capture(state: &AppState, registration: CaptureRegistration) -> Result<()> {
    let mut transaction = state.pool.begin().await?;
    if !matches!(
        registration.assurance_level.as_str(),
        "strongbox" | "tee" | "software_attested" | "web_key"
    ) {
        return Err(anyhow!("unrecognized assurance level"));
    }
    let device_blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blocked_devices WHERE device_fingerprint=$1 AND (expires_at IS NULL OR expires_at>now()))")
        .bind(&registration.device_fingerprint).fetch_one(&mut *transaction).await?;
    if device_blocked {
        return Err(anyhow!("device is blocked by operator policy"));
    }
    if let Some(ip_hash) = &registration.origin_ip_hash {
        let ip_blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blocked_ips WHERE ip_hash=$1 AND (expires_at IS NULL OR expires_at>now()))")
            .bind(ip_hash).fetch_one(&mut *transaction).await?;
        if ip_blocked {
            return Err(anyhow!("origin IP is blocked by operator policy"));
        }
    }
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM captures WHERE device_fingerprint=$1 AND status IN ('initializing','live','stalled') AND id<>$2")
        .bind(&registration.device_fingerprint).bind(&registration.capture_id).fetch_one(&mut *transaction).await?;
    if active >= state.config.max_device_concurrent {
        return Err(anyhow!("device already has an active capture"));
    }
    if let Some(ip_hash) = &registration.origin_ip_hash {
        let active_ip: i64 = sqlx::query_scalar("SELECT count(*) FROM captures WHERE origin_ip_hash=$1 AND status IN ('initializing','live','stalled') AND id<>$2")
            .bind(ip_hash).bind(&registration.capture_id).fetch_one(&mut *transaction).await?;
        if active_ip >= state.config.max_ip_concurrent {
            return Err(anyhow!("origin IP has too many active captures"));
        }
    }
    if registration.streams.iter().any(|stream| {
        !matches!(
            stream.role.as_str(),
            "rear_video" | "front_video" | "audio" | "telemetry"
        )
    }) {
        return Err(anyhow!("capture declared an unsupported stream role"));
    }
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM captures WHERE id=$1)")
        .bind(&registration.capture_id)
        .fetch_one(&mut *transaction)
        .await?
    {
        return Ok(());
    }
    sqlx::query("INSERT INTO captures(id, device_fingerprint, assurance_level, session_public_key_spki, device_public_key_spki, session_binding_signature, upload_token_hash, started_at,origin_ip_hash) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(id) DO NOTHING")
        .bind(&registration.capture_id).bind(&registration.device_fingerprint).bind(&registration.assurance_level)
        .bind(&registration.session_public_key_spki).bind(registration.device_public_key_spki.as_deref().unwrap_or(&registration.session_public_key_spki)).bind(&registration.session_binding_signature)
        .bind(token_hash(&registration.upload_token)).bind(registration.started_at).bind(&registration.origin_ip_hash).execute(&mut *transaction).await?;
    for stream in registration.streams {
        sqlx::query("INSERT INTO streams(id,capture_id,role,mime_type,codec) VALUES($1,$2,$3,$4,$5) ON CONFLICT(id) DO NOTHING")
            .bind(stream.id).bind(&registration.capture_id).bind(stream.role).bind(stream.mime_type).bind(stream.codec).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    emit_event(
        state,
        &registration.capture_id,
        "capture.live",
        json!({"completeness":"pending"}),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicLedgerEvent {
    pub id: String,
    pub capture_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub occurred_at: String,
    pub payload: Value,
    pub signature: String,
}

pub async fn emit_event(
    state: &AppState,
    capture_id: &str,
    event_type: &str,
    payload: Value,
) -> Result<PublicLedgerEvent> {
    let occurred_at = Utc::now();
    let id = Uuid::new_v4();
    let unsigned = json!({"id":id.to_string(),"captureId":capture_id,"type":event_type,"occurredAt":occurred_at.to_rfc3339(),"payload":payload});
    let signature = sign_canonical(&state.signer, &unsigned)?;
    sqlx::query("INSERT INTO evidence_events(id,capture_id,event_type,occurred_at,payload,server_signature) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(capture_id).bind(event_type).bind(occurred_at).bind(&payload).bind(&signature).execute(&state.pool).await?;
    let event = PublicLedgerEvent {
        id: id.to_string(),
        capture_id: capture_id.into(),
        event_type: event_type.into(),
        occurred_at: occurred_at.to_rfc3339(),
        payload,
        signature,
    };
    if let Err(error) = deliver_event(state, &event).await {
        warn!(capture_id, event_type, %error, "ledger event queued for retry");
    }
    Ok(event)
}

pub async fn deliver_event(state: &AppState, event: &PublicLedgerEvent) -> Result<()> {
    let Some(url) = &state.config.ledger_event_url else {
        return Ok(());
    };
    let body = canonical_json(event)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(state.config.internal_secret.as_bytes())?;
    mac.update(body.as_bytes());
    let hmac = hex::encode(mac.finalize().into_bytes());
    let response = state
        .client
        .post(url)
        .header("content-type", "application/json")
        .header("x-proofline-hmac", hmac)
        .body(body)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("ledger returned {}", response.status()));
    }
    sqlx::query("UPDATE evidence_events SET delivered_at=now() WHERE id=$1")
        .bind(Uuid::parse_str(&event.id)?)
        .execute(&state.pool)
        .await?;
    Ok(())
}

pub async fn put_object(state: &AppState, key: &str, bytes: Bytes) -> Result<String> {
    let result = state
        .objects
        .put(&ObjectPath::from(key), bytes.into())
        .await?;
    Ok(result
        .version
        .or(result.e_tag)
        .unwrap_or_else(|| "unversioned".into()))
}

pub async fn get_object(state: &AppState, key: &str) -> Result<Bytes> {
    Ok(state
        .objects
        .get(&ObjectPath::from(key))
        .await?
        .bytes()
        .await?)
}

pub fn simple_pdf(lines: &[String], permalink: Option<&str>) -> Vec<u8> {
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }
    fn wrap(value: &str, width: usize) -> Vec<String> {
        let mut output = Vec::new();
        let mut line = String::new();
        for word in value.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > width {
                output.push(std::mem::take(&mut line));
            }
            if word.len() > width {
                if !line.is_empty() {
                    output.push(std::mem::take(&mut line));
                }
                let mut remaining = word;
                while remaining.len() > width {
                    let split = remaining
                        .char_indices()
                        .map(|(index, _)| index)
                        .take_while(|index| *index <= width)
                        .last()
                        .unwrap_or(width);
                    output.push(remaining[..split].to_string());
                    remaining = &remaining[split..];
                }
                line.push_str(remaining);
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            output.push(line);
        }
        if output.is_empty() {
            output.push(String::new());
        }
        output
    }
    let rows = lines
        .iter()
        .flat_map(|line| wrap(line, 88))
        .take(42)
        .collect::<Vec<_>>();
    let mut content = String::new();
    let mut y = 752;
    for (index, line) in rows.iter().enumerate() {
        let (font, size, color) = if index == 0 {
            ("F2", 15, "0.72 0.08 0.05 rg")
        } else if line.starts_with("ProofLine v2 public evidence report") {
            ("F2", 13, "0 0 0 rg")
        } else {
            ("F1", 10, "0 0 0 rg")
        };
        content.push_str(&format!(
            "BT /{font} {size} Tf {color} 42 {y} Td ({}) Tj ET ",
            escape(line)
        ));
        y -= if index == 0 { 22 } else { 14 };
    }
    content.push_str("ET ");
    if let Some(permalink) = permalink {
        if let Ok(qr) = QrCode::encode_text(permalink, QrCodeEcc::Medium) {
            let module = (126 / qr.size()).max(2);
            let origin_x = 430;
            let origin_y = 38;
            content.push_str("0 0 0 rg ");
            for y in 0..qr.size() {
                for x in 0..qr.size() {
                    if qr.get_module(x, y) {
                        content.push_str(&format!(
                            "{} {} {} {} re f ",
                            origin_x + x * module,
                            origin_y + (qr.size() - 1 - y) * module,
                            module,
                            module
                        ));
                    }
                }
            }
        }
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref
        )
        .as_bytes(),
    );
    pdf
}

#[derive(sqlx::FromRow)]
struct CaptureEvidenceRow {
    status: String,
    completeness: String,
    device_fingerprint: String,
    assurance_level: String,
    session_public_key_spki: String,
    session_binding_signature: String,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    close_reason: Option<String>,
    final_manifest: Option<Value>,
    final_signature: Option<String>,
    tombstone_reason: Option<String>,
}

#[derive(sqlx::FromRow)]
struct FragmentEvidenceRow {
    stream_id: String,
    sequence: i64,
    previous_chain_digest: String,
    media_digest: String,
    chain_digest: String,
    byte_length: i64,
    pts_start_us: i64,
    pts_end_us: i64,
    telemetry_root: String,
    device_signature: String,
    object_key: String,
    object_version: String,
    server_received_at: DateTime<Utc>,
    receipt: Value,
    receipt_signature: String,
}

#[derive(sqlx::FromRow, Serialize)]
struct TelemetryEvidenceRow {
    sequence: i64,
    digest: String,
    previous_digest: String,
    device_signature: String,
    payload: Value,
    received_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
struct AnchorEvidenceRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    merkle_root: String,
    leaf_count: i32,
    leaf_set: Value,
    server_signature: String,
    tsa_url: Option<String>,
    tsa_status: String,
    tsa_response_object_key: Option<String>,
    tsa_error: Option<String>,
}

#[derive(sqlx::FromRow, Serialize)]
struct EventEvidenceRow {
    id: Uuid,
    event_type: String,
    occurred_at: DateTime<Utc>,
    payload: Value,
    server_signature: String,
}

#[derive(sqlx::FromRow, Serialize)]
struct TombstoneEvidenceRow {
    reason: String,
    action_json: Value,
    admin_signature: String,
    created_at: DateTime<Utc>,
}

pub async fn build_evidence_artifacts(state: &AppState, capture_id: &str) -> Result<()> {
    let capture: CaptureEvidenceRow = sqlx::query_as("SELECT status,completeness,device_fingerprint,assurance_level,session_public_key_spki,session_binding_signature,started_at,ended_at,close_reason,final_manifest,final_signature,tombstone_reason FROM captures WHERE id=$1").bind(capture_id).fetch_one(&state.pool).await?;
    let fragments: Vec<FragmentEvidenceRow> = sqlx::query_as("SELECT stream_id,sequence,previous_chain_digest,media_digest,chain_digest,byte_length,pts_start_us,pts_end_us,telemetry_root,device_signature,object_key,object_version,server_received_at,receipt,receipt_signature FROM fragments WHERE capture_id=$1 ORDER BY stream_id,sequence").bind(capture_id).fetch_all(&state.pool).await?;
    let telemetry: Vec<TelemetryEvidenceRow> = sqlx::query_as("SELECT sequence,digest,previous_digest,device_signature,payload,received_at FROM telemetry_batches WHERE capture_id=$1 ORDER BY sequence").bind(capture_id).fetch_all(&state.pool).await?;
    let anchors: Vec<AnchorEvidenceRow> = sqlx::query_as("SELECT DISTINCT a.id,a.created_at,a.merkle_root,a.leaf_count,a.leaf_set,a.server_signature,a.tsa_url,a.tsa_status,a.tsa_response_object_key,a.tsa_error FROM receipt_anchors a JOIN fragments f ON f.anchor_id=a.id WHERE f.capture_id=$1 ORDER BY a.created_at").bind(capture_id).fetch_all(&state.pool).await?;
    let audit_events: Vec<EventEvidenceRow> = sqlx::query_as("SELECT id,event_type,occurred_at,payload,server_signature FROM evidence_events WHERE capture_id=$1 ORDER BY occurred_at,id").bind(capture_id).fetch_all(&state.pool).await?;
    let tombstone: Option<TombstoneEvidenceRow> = sqlx::query_as(
        "SELECT reason,action_json,admin_signature,created_at FROM tombstones WHERE capture_id=$1",
    )
    .bind(capture_id)
    .fetch_optional(&state.pool)
    .await?;
    let tsa_warning = if anchors.is_empty() {
        "No receipt anchor had been created when this report was finalized."
    } else if anchors
        .iter()
        .all(|anchor| anchor.tsa_status == "received_unvalidated")
    {
        "RFC 3161 responses were preserved but must be validated with an independent TSA validator."
    } else if anchors
        .iter()
        .any(|anchor| anchor.tsa_status != "received_unvalidated")
    {
        "One or more receipt anchors lack a successful RFC 3161 response; inspect the per-anchor status."
    } else {
        "Receipt anchor status is available below."
    };
    let report = json!({
        "protocol_version": PROTOCOL_VERSION, "capture_id": capture_id, "status": capture.status, "completeness": capture.completeness,
        "device": {"fingerprint":capture.device_fingerprint,"assurance_level":capture.assurance_level,"session_public_key_spki":capture.session_public_key_spki,"session_binding_signature":capture.session_binding_signature},
        "started_at": capture.started_at, "ended_at": capture.ended_at, "close_reason": capture.close_reason,
        "server": {"receipt_public_key_spki":state.server_public_key_spki},
        "c2pa": {"live_binding":"unsupported_for_current_capture_format","final_asset":"not_generated","note":"The committed official-tool CMAF fixture validates live video and audio bindings. Current Android and PWA fragments are not yet emitted in that validated CMAF shape, and no production signing chain is configured. No proprietary sidecar is represented as C2PA."},
        "fragment_count": fragments.len(), "fragments": fragments.iter().map(|f| json!({"stream_id":f.stream_id,"sequence":f.sequence,"previous_chain_digest":f.previous_chain_digest,"media_digest":f.media_digest,"chain_digest":f.chain_digest,"byte_length":f.byte_length,"pts_start_us":f.pts_start_us,"pts_end_us":f.pts_end_us,"telemetry_root":f.telemetry_root,"device_signature":f.device_signature,"server_received_at":f.server_received_at,"object_key":f.object_key,"object_version":f.object_version,"receipt":f.receipt,"receipt_signature":f.receipt_signature})).collect::<Vec<_>>(),
        "telemetry_batches": telemetry,
        "receipt_anchors": anchors,
        "timestamp_anchor_warning": tsa_warning,
        "device_end_manifest": capture.final_manifest,
        "device_end_signature": capture.final_signature,
        "audit_events": audit_events,
        "tombstone": tombstone,
        "tombstone_reason": capture.tombstone_reason,
        "caveat": "This report verifies received bytes and signed provenance claims. It does not establish that a scene was not staged or that a compromised device reported truthful sensors."
    });
    let report_bytes = Bytes::from(canonical_json(&report)?);
    let report_signature = canonical_json(
        &json!({"algorithm":"ES256","sha256":sha256_hex(&report_bytes),"signature":sign_canonical(&state.signer,&report)?,"server_public_key_spki":state.server_public_key_spki}),
    )?;
    put_object(
        state,
        &format!("evidence/{capture_id}/report.json"),
        report_bytes.clone(),
    )
    .await?;
    let permalink = state
        .config
        .public_web_url
        .as_ref()
        .map(|base| format!("{}/captures/{capture_id}", base.trim_end_matches('/')));
    let pdf = simple_pdf(&[
        "CRITICAL WARNINGS".into(),
        "Provenance does not establish that a scene was not staged or that a compromised device reported truthful sensors.".into(),
        "C2PA live binding: unsupported for the current capture output; no production C2PA claim is made.".into(),
        format!("Timestamp anchors: {}. {}", anchors.len(), tsa_warning),
        format!("Tombstone: {}", capture.tombstone_reason.as_deref().unwrap_or("not tombstoned")),
        "ProofLine v2 public evidence report".into(), format!("Capture: {capture_id}"), format!("Status: {}", capture.status),
        format!("Completeness: {}", capture.completeness), format!("Device: {} ({})", capture.device_fingerprint,capture.assurance_level), format!("Started: {}", capture.started_at),
        format!("Ended: {}", capture.ended_at.map(|v| v.to_rfc3339()).unwrap_or_else(|| "no signed ending".into())),
        format!("Fragments: {}", fragments.len()), format!("Server receipt key: {}",state.server_public_key_spki),
        format!("Canonical evidence JSON SHA-256: {}", sha256_hex(&report_bytes)),
        format!("Permalink: {}", permalink.as_deref().unwrap_or("not configured")),
        "Important: provenance and integrity do not prove the depicted scene is objectively true.".into(),
    ], permalink.as_deref());
    let pdf_signature = canonical_json(
        &json!({"algorithm":"ES256","sha256":sha256_hex(&pdf),"signature":sign_canonical(&state.signer,&json!({"sha256":sha256_hex(&pdf)}))?,"server_public_key_spki":state.server_public_key_spki}),
    )?;
    put_object(
        state,
        &format!("evidence/{capture_id}/report.pdf"),
        Bytes::from(pdf.clone()),
    )
    .await?;
    put_object(
        state,
        &format!("evidence/{capture_id}/report.json.sig"),
        Bytes::from(report_signature.clone()),
    )
    .await?;
    put_object(
        state,
        &format!("evidence/{capture_id}/report.pdf.sig"),
        Bytes::from(pdf_signature.clone()),
    )
    .await?;
    let total: i64 = fragments.iter().map(|fragment| fragment.byte_length).sum();
    if total <= 128 * 1024 * 1024 {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("report.json", options)?;
            zip.write_all(&report_bytes)?;
            zip.start_file("report.json.sig", options)?;
            zip.write_all(report_signature.as_bytes())?;
            zip.start_file("report.pdf", options)?;
            zip.write_all(&pdf)?;
            zip.start_file("report.pdf.sig", options)?;
            zip.write_all(pdf_signature.as_bytes())?;
            for fragment in &fragments {
                zip.start_file(
                    format!(
                        "fragments/{}/{:010}.bin",
                        fragment.stream_id, fragment.sequence
                    ),
                    options,
                )?;
                zip.write_all(&get_object(state, &fragment.object_key).await?)?;
            }
            zip.finish()?;
        }
        put_object(
            state,
            &format!("evidence/{capture_id}/bundle.zip"),
            Bytes::from(cursor.into_inner()),
        )
        .await?;
    }
    sqlx::query("UPDATE captures SET finalized_at=now(), updated_at=now() WHERE id=$1")
        .bind(capture_id)
        .execute(&state.pool)
        .await?;
    info!(capture_id, "evidence artifacts finalized");
    Ok(())
}

pub fn generate_admin_key() -> Result<(String, String)> {
    let signing = SigningKey::random(&mut OsRng);
    let private = URL_SAFE_NO_PAD.encode(signing.to_pkcs8_der()?.as_bytes());
    let public = public_key_spki(signing.verifying_key())?;
    Ok((private, public))
}

pub fn decode_admin_signing_key(encoded: &str) -> Result<SigningKey> {
    Ok(SigningKey::from_pkcs8_der(
        &URL_SAFE_NO_PAD.decode(encoded)?,
    )?)
}

pub fn decode_public_key(encoded: &str) -> Result<VerifyingKey> {
    Ok(VerifyingKey::from_public_key_der(
        &URL_SAFE_NO_PAD.decode(encoded)?,
    )?)
}

pub fn decode_standard_or_url(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| STANDARD.decode(value))
        .map_err(Into::into)
}

/// Builds the append-only receipt-tree root. Leaves are independently hashed first;
/// an odd node is duplicated so verifiers in every language can reproduce the tree.
pub fn merkle_root_hex(leaves: &[Vec<u8>]) -> Result<String> {
    if leaves.is_empty() {
        return Err(anyhow!("cannot build a Merkle root without leaves"));
    }
    let mut level: Vec<Vec<u8>> = leaves
        .iter()
        .map(|leaf| Sha256::digest(leaf).to_vec())
        .collect();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| {
                let right = pair.get(1).unwrap_or(&pair[0]);
                let mut input = Vec::with_capacity(64);
                input.extend_from_slice(&pair[0]);
                input.extend_from_slice(right);
                Sha256::digest(input).to_vec()
            })
            .collect();
    }
    Ok(hex::encode(&level[0]))
}

fn der_wrap(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut output = vec![tag];
    if content.len() < 128 {
        output.push(content.len() as u8);
    } else {
        let length_bytes = (content.len() as u32).to_be_bytes();
        let first = length_bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(length_bytes.len() - 1);
        output.push(0x80 | (length_bytes.len() - first) as u8);
        output.extend_from_slice(&length_bytes[first..]);
    }
    output.extend_from_slice(content);
    output
}

/// Encodes the small RFC 3161 TimeStampReq subset ProofLine needs: SHA-256 message
/// imprint plus certReq=true. The raw response is always retained for independent
/// validation; receiving bytes from a TSA is not itself treated as validation.
pub fn rfc3161_sha256_request(digest_hex: &str) -> Result<Vec<u8>> {
    let digest = hex::decode(digest_hex)?;
    if digest.len() != 32 {
        return Err(anyhow!("RFC 3161 SHA-256 imprint must be 32 bytes"));
    }
    let sha256_oid_and_null = [
        0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00,
    ];
    let algorithm = der_wrap(0x30, &sha256_oid_and_null);
    let digest_octets = der_wrap(0x04, &digest);
    let mut imprint_content = algorithm;
    imprint_content.extend_from_slice(&digest_octets);
    let imprint = der_wrap(0x30, &imprint_content);
    let mut request_content = vec![0x02, 0x01, 0x01];
    request_content.extend_from_slice(&imprint);
    request_content.extend_from_slice(&[0x01, 0x01, 0xff]);
    Ok(der_wrap(0x30, &request_content))
}

#[cfg(test)]
mod evidence_tests {
    use super::*;

    #[test]
    fn rfc3161_request_has_stable_der() {
        let request = rfc3161_sha256_request(&"00".repeat(32)).unwrap();
        assert_eq!(
            hex::encode(request),
            format!(
                "30390201013031300d060960864801650304020105000420{}0101ff",
                "00".repeat(32)
            )
        );
    }

    #[test]
    fn merkle_tree_rejects_empty_and_is_stable() {
        assert!(merkle_root_hex(&[]).is_err());
        assert_eq!(
            merkle_root_hex(&[b"proofline".to_vec()]).unwrap(),
            sha256_hex(b"proofline")
        );
        assert_eq!(
            merkle_root_hex(&[b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]).unwrap(),
            "d31a37ef6ac14a2db1470c4316beb5592e6afd4465022339adafda76a18ffabe"
        );
    }
}
