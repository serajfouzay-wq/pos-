//! Developer helper: sign a license with the committed DEVELOPMENT key.
//!
//! ```text
//! cargo run -p pos-license --features issuer --example issue-dev-license -- <ACTIVATION_CODE>
//! cargo run -p pos-license --features issuer --example issue-dev-license -- --this-machine
//! cargo run -p pos-license --features issuer --example issue-dev-license -- --print-code
//! ```
//!
//! `--print-code` prints this machine's activation code (what the till shows).
//!
//! `--this-machine` fingerprints the current computer for the example client
//! config, so a local `pnpm dev:pos` can be activated without the generator.
//! Tokens from this tool are only accepted by builds embedding the dev key.

use pos_core::config::ClientConfig;
use pos_core::time::{Clock, SystemClock};
use pos_hwid::HardwareComponents;
use pos_license::activation::ActivationRequest;
use pos_license::issuer::{issue_license, IssueOptions, SigningKey};

const DEV_PRIVATE: &str = include_str!("../../../keys/dev/license-dev.private.pem");
const EXAMPLE_CONFIG: &str =
    include_str!("../../../packages/shared/contracts/client-config.example.json");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .ok_or("usage: issue-dev-license <CODE | --this-machine>")?;
    // POS_CLIENT_CONFIG selects another client config (same as the POS build).
    let config_json = match std::env::var("POS_CLIENT_CONFIG") {
        Ok(path) => std::fs::read_to_string(path)?,
        Err(_) => EXAMPLE_CONFIG.to_owned(),
    };
    let config = ClientConfig::parse(&config_json)?;
    let request = if arg == "--this-machine" || arg == "--print-code" {
        let hardware = HardwareComponents::collect()?;
        ActivationRequest {
            client_id: config.client_id,
            fingerprint: hardware.fingerprint(config.client_id).to_string(),
            device_key_hash: hardware.device_key(config.client_id).public_hash(),
            device_name: "dev-machine".into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
        }
    } else {
        ActivationRequest::decode(&arg)?
    };
    if arg == "--print-code" {
        println!("{}", request.encode());
        return Ok(());
    }
    let key = SigningKey::from_unencrypted_pem(DEV_PRIVATE)?;
    let issued = issue_license(
        &key,
        &request,
        &IssueOptions {
            client_slug: config.client_slug,
            business_type: config.business_type,
            max_devices: 5,
            expires_at: None,
        },
        SystemClock.now(),
    )?;
    println!("{}", issued.token);
    Ok(())
}
