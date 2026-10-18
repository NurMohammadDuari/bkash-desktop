//! Native HTTP transport + interactive client for the observed bKash gateways.
//!
//! Default mode is `--dry-run`: it prints exactly what would go on the wire and
//! touches no network. Live mode requires an explicit flag plus a base URL.

use std::sync::Arc;

use anyhow::{bail, Result};
use bkash_native::gateway::{DeviceIdentity, Gateway, HttpRequest, Transport};
use bkash_native::app::http::HttpTransport;
use bkash_native::{domain, VERSION};


fn usage() -> ! {
    eprintln!(
        "bknative {VERSION}

USAGE:
  bknative endpoints                 list every observed gateway + endpoint
  bknative envelope <mobile> <otp>   show the request bodies that would be sent
  bknative live --base <url> ...    perform real calls (explicit opt-in)
"
    );
    std::process::exit(2);
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("endpoints") | None => {
            println!("gateways:");
            for g in domain::GATEWAYS {
                println!("  {g}");
            }
            println!("\nendpoints ({}):", domain::ENDPOINTS.len());
            for e in domain::ENDPOINTS {
                println!("  {e}");
            }
            println!(
                "\nartifact: com.bKash.customerapp 7.3.5 (39008733), Dart package {}, snapshot {}",
                domain::DART_PACKAGE,
                &domain::SNAPSHOT_HASH[..12]
            );
        }
        Some("envelope") => {
            let mobile = args.get(1).cloned().unwrap_or_default();
            let otp = args.get(2).cloned().unwrap_or_default();
            let mobile = domain::normalize_mobile(&mobile).unwrap_or(mobile);
            let gw = Gateway::new(
                Arc::new(DryRun),
                Gateway::default_base().to_string(),
                DeviceIdentity::default(),
            );
            println!("POST {}{}", gw_base(&gw), "otp/send");
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "mobile_number": mobile, "code_length": 6
            }))?);
            println!("\nPOST {}{}", gw_base(&gw), "otp/verify");
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "mobile_number": mobile, "otp": otp
                }))?
            );
        }
        Some("live") => {
            let base = args
                .iter()
                .position(|a| a == "--base")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| Gateway::default_base().to_string());
            if !base.ends_with('/') {
                bail!("base url must end with a slash: {base}");
            }
            let gw = Gateway::new(
                Arc::new(HttpTransport::new()?),
                base.clone(),
                DeviceIdentity::default(),
            );
            tracing::info!(transport = gw.transport_name(), base = %base, "live mode");
            match gw.pre_init() {
                Ok(r) => println!("{}", serde_json::to_string_pretty(&r)?),
                Err(e) => tracing::error!(error = %e, "pre-init failed"),
            }
        }
        _ => usage(),
    }
    Ok(())
}

fn gw_base(_gw: &Gateway) -> &str {
    // `url` is private; reconstruct the documented default for display only.
    &Gateway::default_base()
}

/// Prints requests instead of sending them. The safe default.
struct DryRun;

impl Transport for DryRun {
    fn send(&self, req: HttpRequest) -> Result<String> {
        println!("[dry-run] {} {}", req.method, req.url);
        for (k, v) in &req.headers {
            println!("  {k}: {v}");
        }
        if let Some(b) = &req.body {
            println!("  body: {b}");
        }
        Ok(r#"{"status":"dry-run","success":false,"message":"no network in dry-run"}"#.to_string())
    }

    fn name(&self) -> &'static str {
        "dry-run"
    }
}