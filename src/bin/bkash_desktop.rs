//! `bkash_desktop` — the native bKash client you actually launch.
//!
//!   bkash_desktop              offline demo backend, no account needed
//!   bkash_desktop --live       talk to the recovered bKash gateways
//!
//! Runs on X11 through winit/eframe. No Android, no emulator, no JNI.

use std::sync::Arc;

use bkash_native::app::backend::{Backend, LiveBackend};
use bkash_native::app::ui;

fn usage() -> ! {
    eprintln!(
        "bkash_desktop {}\n\nUSAGE:\n  bkash_desktop            offline demo (default)\n  bkash_desktop --live     live gateway backend\n  bkash_desktop --base URL override the gateway base\n",
        bkash_native::VERSION
    );
    std::process::exit(2);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let live = args.iter().any(|a| a == "--live");
    let base = args
        .iter()
        .position(|a| a == "--base")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| bkash_native::Gateway::default_base().to_string());

    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }

    let backend: Arc<dyn Backend> = if live {
        Arc::new(
            LiveBackend::new(&base).map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?,
        )
    } else {
        ui::simulated()
    };

    ui::run(backend).map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
}
