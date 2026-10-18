//! `bkanalyze` — offline analyzer for the bKash AOT payload. Never executes it.

use std::fs;
use std::path::PathBuf;

use bkash_native::elf::{dart_snapshot_sections, dynamic_symbols};
use bkash_native::features::classify;
use bkash_native::strings::{printable_runs, urls, DEFAULT_MIN_LEN};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: bkanalyze <libapp.so|apk> [--urls] [--symbols] [--min N] [--cap N]")?,
    );
    let mut want_urls = false;
    let mut want_symbols = false;
    let mut want_features = false;
    let mut json_out: Option<String> = None;
    let mut min_len = DEFAULT_MIN_LEN;
    let mut cap = 200_000usize;
    let mut rest = args;
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--urls" => want_urls = true,
            "--features" => want_features = true,
            "--json" => json_out = rest.next(),
            "--symbols" => want_symbols = true,
            "--min" => min_len = rest.next().and_then(|v| v.parse().ok()).unwrap_or(min_len),
            "--cap" => cap = rest.next().and_then(|v| v.parse().ok()).unwrap_or(cap),
            other => return Err(format!("unknown flag {other}").into()),
        }
    }

    let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    println!("artifact : {}", path.display());
    println!("bytes    : {}", bytes.len());
    println!("sha256   : (run `sha256sum {}`)", path.display());

    match dart_snapshot_sections(&bytes) {
        Ok(sections) => {
            println!("\n== Dart AOT snapshot sections ==");
            for s in &sections {
                let desc = match s.header() {
                    Some(h) => format!(
                        "version={} kind={} payload={} hash={}",
                        h.version, h.kind, h.length, h.hash
                    ),
                    None => "instructions section (no data magic)".to_string(),
                };
                println!(
                    "  {:<30} offset=0x{:<10x} size={:<12} {}",
                    s.name, s.offset, s.size, desc
                );
            }
            if let Some(data) = sections
                .iter()
                .find(|s| s.name == "_kDartIsolateSnapshotData")
            {
                println!("  isolate data usable payload: {} bytes", data.size);
            }
        }
        Err(e) => println!("\n== Dart AOT ==\n  not a Dart AOT payload: {e}"),
    }

    if want_symbols {
        println!("\n== dynamic symbols (global/weak, sized) ==");
        match dynamic_symbols(&bytes) {
            Ok(syms) => {
                println!("  count: {}", syms.len());
                for s in syms.iter().take(200) {
                    println!("  0x{:012x} {:>10}  {}", s.value, s.size, s.name);
                }
                if syms.len() > 200 {
                    println!("  ... {} more", syms.len() - 200);
                }
            }
            Err(e) => println!("  failed: {e}"),
        }
    }

    let (runs, truncated) = printable_runs(&bytes, min_len, cap);
    println!(
        "\n== string pool ==\n  printable runs >= {min_len} bytes: {} ({} lower bound)",
        runs.len(),
        if truncated {
            "truncated sample, counts are"
        } else {
            "complete scan, counts are"
        }
    );

    if want_urls {
        let found = urls(&runs);
        println!("  absolute URLs: {}", found.len());
        for u in found
            .iter()
            .filter(|u| u.contains("bka.sh") || u.contains("bkash"))
        {
            println!("    {u}");
        }
    }

    if want_features || json_out.is_some() {
        // Only identifier-shaped tokens are useful for architecture recovery;
        // the 44k printable runs are dominated by asset paths and SQL words.
        let tokens: Vec<String> = runs
            .iter()
            .filter(|s| {
                // `package:` URIs carry slashes and lowercase owner names, so
                // they must bypass the identifier-shaped filter below.
                if s.starts_with("package:") {
                    return s.len() <= 96;
                }
                s.len() <= 64
                    && s.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                    && s.chars().filter(|c| c.is_ascii_uppercase()).count() >= 2
            })
            .cloned()
            .collect();
        let inv = classify(&tokens);
        println!(
            "\n== feature inventory ==\n  identifier tokens: {}",
            tokens.len()
        );
        println!("  i18n keys                : {}", inv.i18n_keys.count);
        println!("  blocs                    : {}", inv.blocs.count);
        println!(
            "  bloc states/events       : {}",
            inv.bloc_states_and_events.count
        );
        println!("  screens/routes           : {}", inv.screens.count);
        println!(
            "  repositories/use cases   : {}",
            inv.repositories_and_use_cases.count
        );
        println!("  vendored packages        : {}", inv.package_uris.len());
        for label in ["screens", "blocs", "repositories_and_use_cases"] {
            let bucket = match label {
                "screens" => &inv.screens,
                "blocs" => &inv.blocs,
                _ => &inv.repositories_and_use_cases,
            };
            if !bucket.samples.is_empty() {
                println!("\n  sample {label}:");
                for s in &bucket.samples {
                    println!("    {s}");
                }
            }
        }
        if !inv.crypto_tokens.is_empty() {
            println!("\n  crypto markers: {}", inv.crypto_tokens.join(", "));
        }
        if let Some(path) = json_out {
            fs::write(&path, serde_json::to_vec_pretty(&inv)?)?;
            println!("\n  wrote {path}");
        }
    }

    Ok(())
}
