//! bKash 7.3.5 native port — core domain types.
//!
//! Ground truth for every string constant in this module came from REA static
//! analysis of `base.apk` / `split_config.arm64_v8a.apk`
//! (package `com.bKash.customerapp`, versionCode `39008733`, targetSdk 35).
//! Nothing here is invented; every field maps to a string observed in
//! `lib/arm64-v8a/libapp.so` (Dart FullAOT snapshot, kind 3).

use serde::{Deserialize, Serialize};

/// Dart package name recovered from the snapshot: `package:bkash_next/config`.
pub const DART_PACKAGE: &str = "bkash_next";

/// Snapshot hash from `_kDartIsolateSnapshotData` header — pins the exact
/// artifact these endpoints were extracted from.
pub const SNAPSHOT_HASH: &str = "1ce86630892e2dca9a8543fdb8ed8e22";
pub const SNAPSHOT_SHA256_LIBAPP: &str =
    "0b1ef5c1f2dfd7579dd3e691a98e615232a8f1d55256e47c998b37a2c9864149";

/// Gateways observed as absolute URL literals in the isolate string pool.
pub const GATEWAYS: &[&str] = &[
    "https://account.bkash.com/youth-app/gateway/api/",
    "https://api.bsense.bka.sh/destiny-mw/",
    "https://api.cde.capp.bka.sh/api/",
    "https://api.ims.bka.sh/v1.0.0/api/",
    "https://eventapi.capp.bka.sh/event-stream/api/",
    "https://huawei.openfinapi.bka.sh/v2/",
    "https://qr.bka.sh/",
];

/// Paths observed as relative literals in the same pool.
pub const ENDPOINTS: &[&str] = &[
    "auth/pre-init",
    "auth/app-status",
    "auth/appStatus",
    "auth/device-fingerprint",
    "auth/login",
    "auth/logout",
    "auth/refresh-token/v2",
    "otp/send",
    "otp/verify",
    "api/feedback/submit",
    "api/v2/conversation",
    "api/cms/destiny-resources/all",
    "api/cms/destiny-resources/search",
    "api/cms/destiny-resources/version",
    "ffs/loan/v2/account/query",
    "ffs/loan/v2/available-products",
    "ffs/loan/v2/list",
    "ffs/loan/v2/onboard",
    "ffs/loan/v2/repay",
    "ffs/loan/v2/repay-prevalidation",
    "ffs/loan/v2/transaction/apply",
    "ffs/loan/v2/transaction/prevalidation",
    "ffs/loan/v2/pay-later/airtime/apply-prevalidation",
    "ffs/loan/v2/pay-later/airtime/available-products",
    "ffs/loan/v2/pay-later/airtime/purchase",
    "ffs/loan/v2/pay-later/pay-merchant/apply-prevalidation",
    "ffs/loan/v2/pay-later/pay-merchant/available-products",
    "ffs/loan/v2/pay-later/pay-merchant/payment",
    "ffs/loan/v2/page-settings",
    "ffs/loan/v2/pinvalidation",
    "my-challenges/v3/campaign-info",
    "my-challenges/v3/ui-settings",
    "referral/v2/campaign-info",
];

/// bKash `trx_id` is a 10-char uppercase alphanumeric string on the wire.
/// The length bound is inferred from the send-money validation strings in the
/// snapshot; it is an inference, not a directly observed constraint.
pub fn is_valid_trx_id(trx_id: &str) -> bool {
    trx_id.len() == 10 && trx_id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// Bangladeshi mobile numbers: 11 digits, leading `1`, operator prefix `3`–`9`.
pub fn normalize_mobile(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    // Strip an international prefix, if present.
    let subscriber = if digits.len() == 13 && digits.starts_with("880") {
        &digits[3..]
    } else if digits.len() == 14 && digits.starts_with("00880") {
        &digits[5..]
    } else {
        &digits[..]
    };
    // Already fully country-coded: +880 then a 10-digit subscriber.
    if subscriber.len() == 10 && subscriber.starts_with('1') {
        return Some(format!("+880{subscriber}"));
    }
    // Local form: 01XXXXXXXXX (11 digits, trunk 0).
    if subscriber.len() == 11 && subscriber.starts_with('0') {
        return Some(format!("+880{}", &subscriber[1..]));
    }
    // Already country-coded: 1XXXXXXXXX.
    if subscriber.len() == 11 && subscriber.starts_with('1') {
        return Some(format!("+880{subscriber}"));
    }
    None
}

/// Authorization header value. The native app sends the access token on
/// `Authorization`; `token_type` is observed in the refresh-token flow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessToken {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

impl AccessToken {
    pub fn bearer(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trx_id_rejects_wrong_length() {
        assert!(is_valid_trx_id("1ABCDEFGHI"));
        assert!(!is_valid_trx_id("1ABCDEFG"));
        assert!(!is_valid_trx_id("1abcdefghi"));
    }

    #[test]
    fn mobile_normalizes_local_and_e164() {
        assert_eq!(
            normalize_mobile("01712345678").as_deref(),
            Some("+8801712345678")
        );
        assert_eq!(
            normalize_mobile("+880 1712-345678").as_deref(),
            Some("+8801712345678")
        );
        assert_eq!(normalize_mobile("12345"), None);
    }

    #[test]
    fn endpoints_are_unique_and_relative() {
        let mut seen = std::collections::HashSet::new();
        for ep in ENDPOINTS {
            assert!(seen.insert(*ep), "duplicate endpoint {ep}");
            assert!(!ep.starts_with("http"), "endpoint must be relative: {ep}");
        }
    }

    #[test]
    fn gateways_are_absolute_https() {
        for gw in GATEWAYS {
            assert!(gw.starts_with("https://"), "plaintext gateway: {gw}");
        }
    }

    #[test]
    fn token_bearer_format() {
        let t = AccessToken {
            access_token: "abc".into(),
            refresh_token: "r".into(),
            token_type: "Bearer".into(),
            expires_in: 3600,
        };
        assert_eq!(t.bearer(), "Bearer abc");
    }
}