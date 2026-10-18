//! bKash 7.3.5 native port library.
//!
//! Two halves, matching the two halves of the target:
//!   * `elf` + `strings` — offline static analysis of the Dart AOT payload.
//!   * `gateway` — a transport-seammed client for the observed REST gateways.
//!
//! All observed constants carry the artifact they came from. Nothing here
//! executes the APK.

pub mod app;
pub mod crypto;
pub mod domain;
pub mod elf;
pub mod features;
pub mod gateway;
pub mod strings;

pub use domain::{AccessToken, DART_PACKAGE, ENDPOINTS, GATEWAYS, SNAPSHOT_HASH};
pub use features::{classify, FeatureInventory};
pub use gateway::{
    DeviceIdentity, Gateway, GatewayResponse, HttpRequest, MockTransport, Transport,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn every_observed_gateway_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for g in GATEWAYS {
            assert!(seen.insert(*g), "duplicate gateway {g}");
        }
    }
}
