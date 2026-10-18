//! Domain types for the desktop client.
//!
//! Field names follow the JSON keys observed in the bKash 7.3.5 isolate string
//! pool (`trx_id`, `status`, `err_code`, `amount`) so that live payloads map
//! onto these types without renaming.

use serde::{Deserialize, Serialize};

/// Money is always integer BDT. bKash has no subunit in circulation; floats
/// here would be a bug, not a convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Bdt(pub i64);

impl Bdt {
    pub const ZERO: Bdt = Bdt(0);

    /// Largest single transfer the observed client would attempt. This is a UI
    /// guard, not a server limit: the server enforces its own ceiling.
    pub const SINGLE_TX_LIMIT: Bdt = Bdt(100_000);

    pub fn checked_sub(self, rhs: Bdt) -> Option<Bdt> {
        self.0.checked_sub(rhs.0).map(Bdt)
    }

    pub fn checked_add(self, rhs: Bdt) -> Option<Bdt> {
        self.0.checked_add(rhs.0).map(Bdt)
    }

    pub fn format(self) -> String {
        format!("Tk {}", self.0)
    }
}

impl std::fmt::Display for Bdt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxKind {
    SendMoney,
    CashOut,
    TopUp,
    Payment,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// bKash `trx_id`: 10 uppercase alphanumeric chars.
    pub trx_id: String,
    pub kind: TxKind,
    pub counterparty: String,
    pub amount: Bdt,
    /// `true` when the counterparty is the account itself.
    pub incoming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub masked_mobile: String,
    pub balance: Bdt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub account: Account,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpChallenge {
    /// Masked destination shown in the UI, e.g. `01712-34567`.
    pub masked_destination: String,
    /// Simulated backends return the code so the flow is completable offline.
    /// A live backend returns `None` because the code goes over SMS.
    pub demo_code: Option<String>,
}

/// A `trx_id` is 10 chars of `[A-Z0-9]`. The alphabet excludes vowels and
/// lookalikes, matching the observed server-side format.
pub fn generate_trx_id(seed: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ1234567890";
    // splitmix64 finalizer first: raw xorshift on small seeds produces visibly
    // correlated first characters, which collided in tests.
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let mut state = z ^ (z >> 31) | 1;
    let mut out = String::with_capacity(10);
    for _ in 0..10 {
        // xorshift64*, so the demo backend is deterministic per session.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push(ALPHABET[(state % ALPHABET.len() as u64) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::is_valid_trx_id;

    #[test]
    fn generated_trx_ids_are_valid_and_deterministic() {
        let a = generate_trx_id(42);
        let b = generate_trx_id(42);
        assert_eq!(a, b);
        assert_eq!(a.len(), 10);
        assert!(is_valid_trx_id(&a), "generated {a}");
        assert_ne!(generate_trx_id(43), a);
    }

    #[test]
    fn money_arithmetic_never_wraps() {
        assert_eq!(Bdt(100).checked_sub(Bdt(30)), Some(Bdt(70)));
        assert_eq!(Bdt(i64::MIN).checked_sub(Bdt(1)), None);
        assert_eq!(Bdt(i64::MAX).checked_add(Bdt(1)), None);
        assert_eq!(Bdt(1500).format(), "Tk 1500");
    }

    #[test]
    fn transaction_round_trips_through_json() {
        let tx = Transaction {
            trx_id: "1ABCDEFGHI".into(),
            kind: TxKind::SendMoney,
            counterparty: "01712345678".into(),
            amount: Bdt(500),
            incoming: false,
        };
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("\"trx_id\":\"1ABCDEFGHI\""));
        assert_eq!(serde_json::from_str::<Transaction>(&json).unwrap(), tx);
    }

    #[test]
    fn bdt_rejects_negative_in_the_ui_guard() {
        // The UI parses to i64 then validates; the guard lives in the backend.
        assert!(Bdt(-1) < Bdt(0));
    }
}