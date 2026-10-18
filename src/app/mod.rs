//! Desktop client: domain model, backend seam, and the egui shell.
//!
//! `bkash_desktop` is the binary; everything here is testable without a window.

pub mod backend;
pub mod http;
pub mod model;
pub mod theme;
pub mod ui;

pub use backend::{Backend, BackendError, LiveBackend, SimulatedBackend};
pub use model::{Account, Bdt, OtpChallenge, Session, Transaction, TxKind};
