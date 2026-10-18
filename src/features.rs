//! Feature inventory extracted from AOT string tokens.
//!
//! The Dart snapshot keeps every identifier as a string constant, so the app's
//! architecture is recoverable by classifying those tokens even though no
//! symbols ship. Buckets come from Flutter/Bloc naming conventions observed in
//! the recovered tokens (`AppStateBloc`, `AmountInputCompleteState`,
//! `AllOffersCompletedPage`, `ADD_MONEY_SUMMARY`).
//!
//! These are naming-convention inferences, not proven type relationships.

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Bucket {
    pub count: usize,
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct FeatureInventory {
    /// `SCREAMING_SNAKE_CASE` localization keys.
    pub i18n_keys: Bucket,
    /// `...Bloc` singletons.
    pub blocs: Bucket,
    /// `...State` / `...Event` bloc members. `...Tapped` is included because this
/// app names interaction events in the past tense (`AmountHotkeyTapped`).
    pub bloc_states_and_events: Bucket,
    /// `...Page` / `...Screen` routes.
    pub screens: Bucket,
    /// `...Repository` / `...UseCase` data layer.
    pub repositories_and_use_cases: Bucket,
    /// `package:` URIs, i.e. the vendored Dart packages.
    pub package_uris: Vec<String>,
    /// Tokens that pin the transport crypto scheme.
    pub crypto_tokens: Vec<String>,
    pub total_tokens: usize,
}

const MAX_SAMPLES: usize = 12;

fn is_screaming_snake(t: &str) -> bool {
    !t.is_empty()
        && t.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && t.chars().any(|c| c.is_ascii_uppercase())
}

fn push(bucket: &mut Bucket, token: &str) {
    bucket.count += 1;
    if bucket.samples.len() < MAX_SAMPLES {
        bucket.samples.push(token.to_string());
    }
}

fn ends_with_any(token: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|s| token.ends_with(s))
}

/// Tokens whose presence fixes the request-encryption scheme. Each is an exact
/// string observed in `libapp.so`.
pub const CRYPTO_MARKERS: &[&str] = &[
    "AES_CBC_PKCS7Padding",
    "RSAES-OAEP",
    "FlutterEncryptedStorage",
    "aesKey",
    "publicKey",
];

pub fn classify(tokens: &[String]) -> FeatureInventory {
    let mut inv = FeatureInventory {
        total_tokens: tokens.len(),
        ..Default::default()
    };
    let mut packages: BTreeMap<String, ()> = BTreeMap::new();

    for token in tokens {
        if let Some(uri) = token.strip_prefix("package:") {
            // A package URI is `package:<name>/<path...>`, so the package name
            // is the first segment only: `package:flutter/src/dart_ui/x.dart`
            // belongs to `flutter`, not to `flutter/src`.
            if let Some(name) = uri.split('/').next().filter(|n| !n.is_empty()) {
                packages.insert(name.to_string(), ());
            }
        }
        if CRYPTO_MARKERS.contains(&token.as_str()) {
            inv.crypto_tokens.push(token.clone());
        }
        if is_screaming_snake(token) {
            push(&mut inv.i18n_keys, token);
            continue;
        }
        if !token.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            continue;
        }
        if ends_with_any(token, &["Bloc", "Cubit"]) {
            push(&mut inv.blocs, token);
        } else if ends_with_any(token, &["State", "Event", "Status", "Tapped", "Clicked"]) {
            push(&mut inv.bloc_states_and_events, token);
        } else if ends_with_any(token, &["Page", "Screen", "Route"]) {
            push(&mut inv.screens, token);
        } else if ends_with_any(token, &["Repository", "UseCase", "DataSource", "Service"]) {
            push(&mut inv.repositories_and_use_cases, token);
        }
    }

    inv.package_uris = packages.into_keys().collect();
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classifies_each_architecture_bucket() {
        let inv = classify(&v(&[
            "AppStateBloc",
            "AmountInputCompleteState",
            "AmountInputStartedState",
            "AmountHotkeyTapped",
            "AllOffersCompletedPage",
            "AmountInputScreen",
            "UserRepository",
            "SendMoneyUseCase",
            "ADD_MONEY_SUMMARY",
            "AES_CBC_PKCS7Padding",
            "package:bkash_next/config/env.dart",
            "dart:_http",
            "lowercaseThing",
        ]));
        assert_eq!(inv.blocs.count, 1);
        assert_eq!(inv.bloc_states_and_events.count, 3);
        assert_eq!(inv.screens.count, 2);
        assert_eq!(inv.repositories_and_use_cases.count, 2);
        assert_eq!(inv.i18n_keys.count, 1);
        assert_eq!(inv.i18n_keys.samples[0], "ADD_MONEY_SUMMARY");
        assert_eq!(inv.crypto_tokens, vec!["AES_CBC_PKCS7Padding"]);
        assert_eq!(inv.package_uris, vec!["bkash_next"]);
        assert_eq!(inv.total_tokens, 13);
    }

    #[test]
    fn screaming_snake_needs_uppercase_and_is_not_lowercase() {
        assert!(is_screaming_snake("ADD_MONEY_SUMMARY"));
        assert!(is_screaming_snake("OTP_2"));
        assert!(!is_screaming_snake("lowercase"));
        assert!(!is_screaming_snake("__"));
        assert!(!is_screaming_snake(""));
    }

    #[test]
    fn samples_are_capped_but_counts_keep_climbing() {
        let many: Vec<String> = (0..40).map(|i| format!("Screen{i}Page")).collect();
        let inv = classify(&many);
        assert_eq!(inv.screens.count, 40);
        assert_eq!(inv.screens.samples.len(), MAX_SAMPLES);
    }

    #[test]
    fn package_uris_are_deduped_and_truncated_to_owner_repo() {
        let inv = classify(&v(&[
            "package:bkash_next/config/env.dart",
            "package:bkash_next/config/env_production.dart",
            "package:flutter/src/dart_ui/x.dart",
            "dart:convert",
        ]));
        assert_eq!(inv.package_uris, vec!["bkash_next", "flutter"]);
    }

    #[test]
    fn empty_input_yields_empty_inventory() {
        assert_eq!(classify(&[]), FeatureInventory::default());
    }
}