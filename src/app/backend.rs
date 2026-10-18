//! Backend seam: the UI never touches HTTP.
//!
//! Two implementations ship:
//!   * [`SimulatedBackend`] — deterministic, offline, fully exercisable. This
//!     is what runs by default so the app is demonstrable with no account.
//!   * [`LiveBackend`] — speaks the recovered bKash gateways through
//!     [`bkash_native::gateway::Gateway`]. The auth handshake was never
//!     captured live, so `LiveBackend` reports that honestly rather than
//!     pretending a token it did not receive.

use std::sync::{Arc, Mutex};

use super::http::HttpTransport;
use crate::domain::normalize_mobile;
use crate::gateway::{DeviceIdentity, Gateway, Transport};

use super::model::{generate_trx_id, Account, Bdt, OtpChallenge, Session, Transaction, TxKind};

/// Errors a user can act on. Anything else is a bug.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    #[error("enter a valid 11-digit Bangladeshi mobile number")]
    BadMobile,
    #[error("that OTP is not 6 digits")]
    BadOtp,
    #[error("wrong OTP — try the code shown on screen")]
    WrongOtp,
    #[error("enter an amount greater than zero")]
    BadAmount,
    #[error("amount exceeds the Tk 100,000 single-transfer limit")]
    AmountTooLarge,
    #[error("not enough balance for that transfer")]
    InsufficientFunds,
    #[error("account has no money yet")]
    NoFunds,
    #[error("live gateway call failed: {0}")]
    Live(String),
    #[error("this build has no verified live session for your account")]
    LiveUnavailable,
}

pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    fn request_otp(&self, mobile: &str) -> Result<OtpChallenge, BackendError>;
    fn verify_otp(&self, mobile: &str, otp: &str) -> Result<Session, BackendError>;
    fn balance(&self, session: &Session) -> Result<Bdt, BackendError>;
    fn send_money(
        &self,
        session: &Session,
        to_mobile: &str,
        amount: Bdt,
    ) -> Result<Transaction, BackendError>;
    fn cash_out(
        &self,
        session: &Session,
        agent_mobile: &str,
        amount: Bdt,
    ) -> Result<Transaction, BackendError>;
    fn history(&self, session: &Session) -> Result<Vec<Transaction>, BackendError>;
}

fn validate_amount(amount: Bdt, balance: Bdt) -> Result<(), BackendError> {
    if amount <= Bdt::ZERO {
        return Err(BackendError::BadAmount);
    }
    if amount > Bdt::SINGLE_TX_LIMIT {
        return Err(BackendError::AmountTooLarge);
    }
    if amount > balance {
        return Err(BackendError::InsufficientFunds);
    }
    Ok(())
}

/// Fully offline backend. Deterministic: the same inputs always produce the
/// same transactions, which makes the UI testable.
pub struct SimulatedBackend {
    state: Mutex<SimState>,
}

#[derive(Default)]
struct SimState {
    balance: Bdt,
    history: Vec<Transaction>,
    counter: u64,
    last_otp: String,
    signed_in: Option<Session>,
}

impl Default for SimulatedBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatedBackend {
    pub fn new() -> Self {
        let mut history = Vec::new();
        let seed_rows: &[(TxKind, &str, i64, bool)] = &[
            (TxKind::CashOut, "01712345678", 2_000, false),
            (TxKind::SendMoney, "01812345678", 500, false),
            (TxKind::TopUp, "Bkash Topup", 1_000, true),
            (TxKind::Payment, "Daraz", 1_299, false),
        ];
        let mut counter = 7u64;
        for (kind, who, amount, incoming) in seed_rows.iter().copied() {
            history.push(Transaction {
                trx_id: generate_trx_id(counter),
                kind,
                counterparty: who.to_string(),
                amount: Bdt(amount),
                incoming,
            });
            counter += 13;
        }
        history.reverse();
        Self {
            state: Mutex::new(SimState {
                balance: Bdt(12_450),
                history,
                counter,
                ..Default::default()
            }),
        }
    }

    fn record(&self, kind: TxKind, who: &str, amount: Bdt, incoming: bool) -> Transaction {
        let mut st = self.state.lock().expect("sim state lock");
        st.counter += 1;
        let tx = Transaction {
            trx_id: generate_trx_id(st.counter),
            kind,
            counterparty: who.to_string(),
            amount,
            incoming,
        };
        st.balance = if incoming {
            st.balance.checked_add(amount).unwrap_or(st.balance)
        } else {
            st.balance.checked_sub(amount).unwrap_or(Bdt::ZERO)
        };
        st.history.insert(0, tx.clone());
        st.history.truncate(50);
        tx
    }
}

impl Backend for SimulatedBackend {
    fn name(&self) -> &'static str {
        "simulated"
    }

    fn request_otp(&self, mobile: &str) -> Result<OtpChallenge, BackendError> {
        let e164 = normalize_mobile(mobile).ok_or(BackendError::BadMobile)?;
        let digits: String = e164.chars().filter(char::is_ascii_digit).collect();
        // Derive a stable 6-digit code so repeat logins are reproducible.
        let seed: u64 = digits.bytes().map(|b| b as u64).sum();
        let code = format!("{:06}", seed % 1_000_000);
        self.state.lock().expect("sim state lock").last_otp = code.clone();
        Ok(OtpChallenge {
            masked_destination: format!(
                "{}-{}",
                &digits[digits.len() - 11..digits.len() - 7],
                &digits[digits.len() - 7..]
            ),
            demo_code: Some(code),
        })
    }

    fn verify_otp(&self, mobile: &str, otp: &str) -> Result<Session, BackendError> {
        normalize_mobile(mobile).ok_or(BackendError::BadMobile)?;
        let otp = otp.trim();
        if otp.len() != 6 || !otp.chars().all(|c| c.is_ascii_digit()) {
            return Err(BackendError::BadOtp);
        }
        let mut st = self.state.lock().expect("sim state lock");
        if otp != st.last_otp {
            return Err(BackendError::WrongOtp);
        }
        let digits: String = normalize_mobile(mobile)
            .unwrap_or_default()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        let session = Session {
            account: Account {
                name: "Sam".into(),
                masked_mobile: format!("+880*{}", &digits[digits.len() - 4..]),
                balance: st.balance,
            },
            token: format!("demo-{}", generate_trx_id(st.counter)),
        };
        st.signed_in = Some(session.clone());
        Ok(session)
    }

    fn balance(&self, _session: &Session) -> Result<Bdt, BackendError> {
        Ok(self.state.lock().expect("sim state lock").balance)
    }

    fn send_money(
        &self,
        _s: &Session,
        to_mobile: &str,
        amount: Bdt,
    ) -> Result<Transaction, BackendError> {
        let to = normalize_mobile(to_mobile).ok_or(BackendError::BadMobile)?;
        validate_amount(
            amount,
            self.balance(&Session {
                account: Account {
                    name: String::new(),
                    masked_mobile: String::new(),
                    balance: Bdt::ZERO,
                },
                token: String::new(),
            })
            .unwrap_or(Bdt::ZERO),
        )
        .or_else(|e| match e {
            BackendError::InsufficientFunds => Err(BackendError::InsufficientFunds),
            other => Err(other),
        })?;
        Ok(self.record(TxKind::SendMoney, &to, amount, false))
    }

    fn cash_out(
        &self,
        _s: &Session,
        agent: &str,
        amount: Bdt,
    ) -> Result<Transaction, BackendError> {
        let a = normalize_mobile(agent).ok_or(BackendError::BadMobile)?;
        let bal = self.state.lock().expect("sim state lock").balance;
        if bal <= Bdt::ZERO {
            return Err(BackendError::NoFunds);
        }
        validate_amount(amount, bal)?;
        Ok(self.record(TxKind::CashOut, &a, amount, false))
    }

    fn history(&self, _session: &Session) -> Result<Vec<Transaction>, BackendError> {
        Ok(self.state.lock().expect("sim state lock").history.clone())
    }
}

/// Live backend against the recovered gateways.
pub struct LiveBackend {
    gateway: Mutex<Gateway>,
    transport: Arc<dyn Transport>,
}

impl LiveBackend {
    pub fn new(base_url: &str) -> Result<Self, BackendError> {
        let transport: Arc<dyn Transport> =
            Arc::new(HttpTransport::new().map_err(|e| BackendError::Live(e.to_string()))?);
        Ok(Self {
            gateway: Mutex::new(Gateway::new(
                transport.clone(),
                base_url,
                DeviceIdentity::default(),
            )),
            transport,
        })
    }

    /// Exposed so the UI can show exactly what would be sent. Used by tests.
    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.transport
    }
}

impl Backend for LiveBackend {
    fn name(&self) -> &'static str {
        "live"
    }

    fn request_otp(&self, mobile: &str) -> Result<OtpChallenge, BackendError> {
        let e164 = normalize_mobile(mobile).ok_or(BackendError::BadMobile)?;
        let gw = self.gateway.lock().expect("gateway lock");
        gw.send_otp(&e164, 6)
            .map_err(|e| BackendError::Live(e.to_string()))?;
        Ok(OtpChallenge {
            masked_destination: format!("+880*{}", &e164[e164.len() - 4..]),
            demo_code: None,
        })
    }

    fn verify_otp(&self, _mobile: &str, _otp: &str) -> Result<Session, BackendError> {
        // The handshake that turns an OTP into a session was never captured
        // from a real device, so this is refused rather than faked.
        Err(BackendError::LiveUnavailable)
    }

    fn balance(&self, _session: &Session) -> Result<Bdt, BackendError> {
        Err(BackendError::LiveUnavailable)
    }

    fn send_money(
        &self,
        _s: &Session,
        _to: &str,
        _amount: Bdt,
    ) -> Result<Transaction, BackendError> {
        Err(BackendError::LiveUnavailable)
    }

    fn cash_out(&self, _s: &Session, _a: &str, _amount: Bdt) -> Result<Transaction, BackendError> {
        Err(BackendError::LiveUnavailable)
    }

    fn history(&self, _s: &Session) -> Result<Vec<Transaction>, BackendError> {
        Err(BackendError::LiveUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_session(b: &SimulatedBackend) -> Session {
        b.request_otp("01712345678").unwrap();
        b.verify_otp("01712345678", &demo_code(b)).unwrap()
    }

    fn demo_code(b: &SimulatedBackend) -> String {
        b.state.lock().unwrap().last_otp.clone()
    }

    #[test]
    fn otp_flow_validates_mobile_and_code() {
        let b = SimulatedBackend::new();
        assert_eq!(b.request_otp("12345").unwrap_err(), BackendError::BadMobile);
        // 01712-345678 is 11 digits and valid; 12345 is not.
        assert!(b.request_otp("01712-345678").is_ok());
        assert_eq!(b.request_otp("12345").unwrap_err(), BackendError::BadMobile);
        let challenge = b.request_otp("01712345678").unwrap();
        assert!(challenge.demo_code.is_some());
        assert!(challenge.masked_destination.contains("3456"));
        assert_eq!(
            b.verify_otp("01712345678", "000000").unwrap_err(),
            BackendError::WrongOtp
        );
        assert_eq!(
            b.verify_otp("01712345678", "12").unwrap_err(),
            BackendError::BadOtp
        );
        let s = b.verify_otp("01712345678", &demo_code(&b)).unwrap();
        assert!(s.token.starts_with("demo-"));
    }

    #[test]
    fn send_money_moves_balance_and_appends_history() {
        let b = SimulatedBackend::new();
        let s = demo_session(&b);
        let before = b.balance(&s).unwrap();
        let tx = b.send_money(&s, "01812345678", Bdt(500)).unwrap();
        assert_eq!(tx.amount, Bdt(500));
        assert!(!tx.incoming);
        assert_eq!(crate::domain::is_valid_trx_id(&tx.trx_id), true);
        assert_eq!(b.balance(&s).unwrap(), Bdt(before.0 - 500));
        assert_eq!(b.history(&s).unwrap()[0].trx_id, tx.trx_id);
    }

    #[test]
    fn amount_guards_reject_bad_values() {
        let b = SimulatedBackend::new();
        let s = demo_session(&b);
        assert_eq!(
            b.send_money(&s, "01812345678", Bdt(0)).unwrap_err(),
            BackendError::BadAmount
        );
        assert_eq!(
            b.send_money(&s, "01812345678", Bdt(200_000)).unwrap_err(),
            BackendError::AmountTooLarge
        );
        assert_eq!(
            b.send_money(&s, "01812345678", Bdt(999_999)).unwrap_err(),
            BackendError::AmountTooLarge
        );
    }

    #[test]
    fn insufficient_funds_is_enforced() {
        let b = SimulatedBackend::new();
        let s = demo_session(&b);
        let balance = b.balance(&s).unwrap();
        assert_eq!(
            b.send_money(&s, "01812345678", Bdt(balance.0 + 1))
                .unwrap_err(),
            BackendError::InsufficientFunds
        );
    }

    #[test]
    fn cash_out_requires_a_funded_account_and_valid_agent() {
        let b = SimulatedBackend::new();
        let s = demo_session(&b);
        assert_eq!(
            b.cash_out(&s, "123", Bdt(100)).unwrap_err(),
            BackendError::BadMobile
        );
        assert!(b.cash_out(&s, "01812345678", Bdt(1_000)).is_ok());
    }

    #[test]
    fn live_backend_refuses_to_fake_a_session() {
        let b = LiveBackend::new(crate::Gateway::default_base()).unwrap();
        assert_eq!(b.name(), "live");
        assert_eq!(
            b.verify_otp("01712345678", "123456").unwrap_err(),
            BackendError::LiveUnavailable
        );
    }

    #[test]
    fn seeded_history_is_newest_first_and_bounded() {
        let b = SimulatedBackend::new();
        let s = demo_session(&b);
        let h = b.history(&s).unwrap();
        assert!(h.len() >= 4);
        assert_eq!(h[0].kind, TxKind::Payment);
        assert!(h
            .iter()
            .take(50)
            .all(|t| crate::domain::is_valid_trx_id(&t.trx_id)));
    }
}
