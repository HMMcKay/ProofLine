use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{
    Signature, SigningKey, VerifyingKey,
    signature::{Signer, Verifier},
};
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROTOCOL_VERSION: &str = "proofline/2";
pub const GENESIS_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid base64url value")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid P-256 public key")]
    PublicKey,
    #[error("invalid P-256 signature")]
    Signature,
    #[error("canonical JSON serialization failed")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssuranceLevel {
    Strongbox,
    Tee,
    SoftwareAttested,
    WebKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Initializing,
    Live,
    Stalled,
    Sealed,
    Interrupted,
    Tombstoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessClaim {
    CompleteWithSignedEnd,
    CompleteAsServerReceived,
    GapsDetected,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentEnvelope {
    pub protocol_version: String,
    pub capture_id: String,
    pub stream_id: String,
    pub sequence: i64,
    pub previous_chain_digest: String,
    pub media_digest: String,
    pub chain_digest: String,
    pub byte_length: i64,
    pub pts_start_us: i64,
    pub pts_end_us: i64,
    pub telemetry_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentChainInput {
    pub protocol_version: String,
    pub capture_id: String,
    pub stream_id: String,
    pub sequence: i64,
    pub previous_chain_digest: String,
    pub pts_start_us: i64,
    pub pts_end_us: i64,
    pub telemetry_root: String,
    pub media_digest: String,
    pub byte_length: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerReceipt {
    pub protocol_version: String,
    pub capture_id: String,
    pub stream_id: String,
    pub sequence: i64,
    pub media_digest: String,
    pub chain_digest: String,
    pub previous_chain_digest: String,
    pub byte_length: i64,
    pub server_received_at: String,
    pub object_key: String,
    pub object_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedReceipt {
    pub receipt: ServerReceipt,
    pub signature: String,
    pub server_public_key_spki: String,
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            let mut result = Map::new();
            for key in keys {
                result.insert(key.clone(), sorted(&object[key]));
            }
            Value::Object(result)
        }
        primitive => primitive.clone(),
    }
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<String, ProtocolError> {
    Ok(serde_json::to_string(&sorted(&serde_json::to_value(
        value,
    )?))?)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn expected_chain_digest(envelope: &FragmentEnvelope) -> Result<String, ProtocolError> {
    let input = FragmentChainInput {
        protocol_version: envelope.protocol_version.clone(),
        capture_id: envelope.capture_id.clone(),
        stream_id: envelope.stream_id.clone(),
        sequence: envelope.sequence,
        previous_chain_digest: envelope.previous_chain_digest.clone(),
        pts_start_us: envelope.pts_start_us,
        pts_end_us: envelope.pts_end_us,
        telemetry_root: envelope.telemetry_root.clone(),
        media_digest: envelope.media_digest.clone(),
        byte_length: envelope.byte_length,
    };
    Ok(sha256_hex(canonical_json(&input)?.as_bytes()))
}

pub fn verifying_key_from_spki(encoded: &str) -> Result<VerifyingKey, ProtocolError> {
    let der = URL_SAFE_NO_PAD.decode(encoded)?;
    VerifyingKey::from_public_key_der(&der).map_err(|_| ProtocolError::PublicKey)
}

pub fn public_key_spki(key: &VerifyingKey) -> Result<String, ProtocolError> {
    let der = key
        .to_public_key_der()
        .map_err(|_| ProtocolError::PublicKey)?;
    Ok(URL_SAFE_NO_PAD.encode(der.as_bytes()))
}

pub fn verify_canonical<T: Serialize>(
    key: &VerifyingKey,
    value: &T,
    encoded_signature: &str,
) -> Result<(), ProtocolError> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded_signature)?;
    let signature = Signature::from_slice(&bytes)
        .or_else(|_| Signature::from_der(&bytes))
        .map_err(|_| ProtocolError::Signature)?;
    key.verify(canonical_json(value)?.as_bytes(), &signature)
        .map_err(|_| ProtocolError::Signature)
}

pub fn sign_canonical<T: Serialize>(key: &SigningKey, value: &T) -> Result<String, ProtocolError> {
    let signature: Signature = key.sign(canonical_json(value)?.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn canonicalization_sorts_nested_objects() {
        let value = serde_json::json!({"z": 1, "a": {"y": 2, "b": 3}});
        assert_eq!(
            canonical_json(&value).unwrap(),
            r#"{"a":{"b":3,"y":2},"z":1}"#
        );
    }

    #[test]
    fn signatures_round_trip() {
        let signing = SigningKey::random(&mut OsRng);
        let value = serde_json::json!({"captureId": "cap_test", "sequence": 0});
        let signature = sign_canonical(&signing, &value).unwrap();
        verify_canonical(signing.verifying_key(), &value, &signature).unwrap();
    }

    #[test]
    fn shared_fragment_vector_matches() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../protocol/test-vectors/fragment-chain-v2.json"
        ))
        .unwrap();
        let input = &fixture["chain_input"];
        let canonical = canonical_json(input).unwrap();
        assert_eq!(canonical, fixture["canonical"].as_str().unwrap());
        assert_eq!(
            sha256_hex(canonical.as_bytes()),
            fixture["chain_digest"].as_str().unwrap()
        );
    }
}
