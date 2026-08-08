use chrono::Utc;
use clap::{Parser, Subcommand};
use proofline_media::{decode_admin_signing_key, generate_admin_key};
use proofline_protocol::sign_canonical;
use serde::Serialize;
use std::{fs, path::PathBuf};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "proofline-admin",
    about = "Offline-signed ProofLine administrative actions"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Keygen {
        #[arg(long)]
        private_key_out: PathBuf,
    },
    Tombstone {
        #[arg(long)]
        gateway: String,
        #[arg(long)]
        capture: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        private_key: PathBuf,
        #[arg(long, env = "PROOFLINE_INTERNAL_SECRET")]
        internal_secret: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TombstoneAction {
    capture_id: String,
    reason: String,
    issued_at: String,
    nonce: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Keygen { private_key_out } => {
            let (private, public) = generate_admin_key()?;
            fs::write(&private_key_out, private)?;
            println!("Admin public key SPKI (configure on the gateway): {public}");
            println!(
                "Private key written to {}. Store it offline; it cannot be recovered.",
                private_key_out.display()
            );
        }
        Command::Tombstone {
            gateway,
            capture,
            reason,
            private_key,
            internal_secret,
        } => {
            let key = decode_admin_signing_key(fs::read_to_string(private_key)?.trim())?;
            let action = TombstoneAction {
                capture_id: capture,
                reason,
                issued_at: Utc::now().to_rfc3339(),
                nonce: Uuid::new_v4().to_string(),
            };
            let signature = sign_canonical(&key, &action)?;
            let response = reqwest::Client::new()
                .post(format!(
                    "{}/internal/v1/tombstones",
                    gateway.trim_end_matches('/')
                ))
                .header("x-proofline-internal-secret", internal_secret)
                .header("x-proofline-admin-signature", signature)
                .json(&action)
                .send()
                .await?;
            if !response.status().is_success() {
                anyhow::bail!(
                    "gateway rejected tombstone: {} {}",
                    response.status(),
                    response.text().await?
                );
            }
            println!(
                "The capture is tombstoned. Media playback is hidden; hashes and the signed action remain."
            );
        }
    }
    Ok(())
}
