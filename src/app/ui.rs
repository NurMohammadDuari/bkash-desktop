//! egui desktop shell.
//!
//! The whole UI is a state machine over a [`Backend`]. Backend calls run on a
//! worker thread and come back through a channel, so a slow gateway can never
//! freeze the window.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use egui::{Align, RichText};

use crate::app::backend::{Backend, BackendError, SimulatedBackend};
use crate::app::model::{Bdt, OtpChallenge, Session, Transaction, TxKind};
use crate::app::theme::{
    meta_label, nav_item, primary_button, secondary_button, section_label, title, Theme,
};

/// Which page the user is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Login,
    Home,
    Send,
    CashOut,
    History,
}

/// A unit of work pushed to the worker thread.
enum Job {
    RequestOtp { mobile: String },
    VerifyOtp { mobile: String, otp: String },
    Send { to: String, amount: Bdt },
    CashOut { agent: String, amount: Bdt },
    Refresh,
}

/// The reply the worker sends back.
enum Reply {
    Otp(OtpChallenge),
    LoggedIn(Box<Session>),
    Balance(Bdt),
    Tx(Box<Transaction>),
    History(Vec<Transaction>),
    Err(BackendError),
}

impl eframe::App for BkashApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::dark();
        if !self.themed {
            theme.install(ctx);
            self.themed = true;
        }
        theme.paint_background(ctx);
        self.drain();
        self.chrome(ctx, &theme);
    }
}

pub struct BkashApp {
    backend: Arc<dyn Backend>,
    themed: bool,
    screen: Screen,
    mobile: String,
    otp: String,
    amount: String,
    counterparty: String,
    challenge: Option<OtpChallenge>,
    session: Option<Session>,
    history: Vec<Transaction>,
    status: Option<(String, bool)>,
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
    busy: Arc<Mutex<bool>>,
    /// Shared with the worker thread so transfer jobs carry the real session.
    session_slot: Arc<Mutex<Option<Session>>>,
}

impl BkashApp {
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (reply_tx, replies) = channel::<Reply>();
        let busy = Arc::new(Mutex::new(false));
        let session_slot: Arc<Mutex<Option<Session>>> = Arc::new(Mutex::new(None));

        let worker_backend = Arc::clone(&backend);
        let worker_busy = Arc::clone(&busy);
        let worker_session = Arc::clone(&session_slot);
        std::thread::spawn(move || {
            for job in job_rx {
                *worker_busy.lock().expect("busy lock") = true;
                let reply = match &job {
                    Job::RequestOtp { mobile } => worker_backend
                        .request_otp(mobile)
                        .map(Reply::Otp)
                        .unwrap_or_else(Reply::Err),
                    Job::VerifyOtp { mobile, otp } => worker_backend
                        .verify_otp(mobile, otp)
                        .map(|s| Reply::LoggedIn(Box::new(s)))
                        .unwrap_or_else(Reply::Err),
                    Job::Send { to, amount } => {
                        let session = worker_session.lock().expect("session slot").clone();
                        match session {
                            Some(s) => worker_backend
                                .send_money(&s, to, *amount)
                                .map(|t| Reply::Tx(Box::new(t)))
                                .unwrap_or_else(Reply::Err),
                            None => Reply::Err(BackendError::LiveUnavailable),
                        }
                    }
                    Job::CashOut { agent, amount } => {
                        let session = worker_session.lock().expect("session slot").clone();
                        match session {
                            Some(s) => worker_backend
                                .cash_out(&s, agent, *amount)
                                .map(|t| Reply::Tx(Box::new(t)))
                                .unwrap_or_else(Reply::Err),
                            None => Reply::Err(BackendError::LiveUnavailable),
                        }
                    }
                    Job::Refresh => match worker_session.lock().expect("session slot").clone() {
                        Some(s) => match (worker_backend.balance(&s), worker_backend.history(&s)) {
                            (Ok(b), Ok(h)) => {
                                let _ = reply_tx.send(Reply::Balance(b));
                                Reply::History(h)
                            }
                            (Err(e), _) | (_, Err(e)) => Reply::Err(e),
                        },
                        None => Reply::Err(BackendError::LiveUnavailable),
                    },
                };
                *worker_busy.lock().expect("busy lock") = false;
                let _ = reply_tx.send(reply);
            }
        });

        Self {
            backend,
            themed: false,
            screen: Screen::Login,
            mobile: String::new(),
            otp: String::new(),
            amount: String::new(),
            counterparty: String::new(),
            challenge: None,
            session: None,
            history: Vec::new(),
            status: None,
            jobs,
            replies,
            busy,
            session_slot,
        }
    }

    fn submit(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    fn drain(&mut self) {
        while let Ok(reply) = self.replies.try_recv() {
            match reply {
                Reply::Otp(c) => {
                    self.challenge = Some(c);
                    self.status = Some(("OTP sent".into(), true));
                }
                Reply::LoggedIn(s) => {
                    *self.session_slot.lock().expect("session slot") = Some((*s).clone());
                    self.session = Some(*s);
                    self.screen = Screen::Home;
                    self.submit(Job::Refresh);
                    self.status = Some(("Signed in".into(), true));
                }
                Reply::Balance(b) => {
                    if let Some(s) = self.session.as_mut() {
                        s.account.balance = b;
                    }
                }
                Reply::Tx(t) => {
                    self.status = Some((format!("{} · {}", t.trx_id, t.amount.format()), true));
                    self.submit(Job::Refresh);
                }
                Reply::History(h) => self.history = h,
                Reply::Err(e) => self.status = Some((e.to_string(), false)),
            }
        }
    }

    fn parse_amount(&self) -> Result<Bdt, BackendError> {
        let t = self.amount.trim();
        if t.is_empty() {
            return Err(BackendError::BadAmount);
        }
        t.parse::<i64>()
            .map(Bdt)
            .map_err(|_| BackendError::BadAmount)
    }

    /// Top bar + sidebar + content.
    fn chrome(&mut self, ctx: &egui::Context, t: &Theme) {
        let signed_in = self.session.is_some();

        egui::SidePanel::left("nav")
            .exact_width(232.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(t.bg_top)
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                t.sidebar_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("bKash").size(22.0).color(t.accent).strong());
                        ui.label(RichText::new("desktop").size(12.0).color(t.text_dim));
                    });
                    ui.add_space(18.0);
                    if signed_in {
                        if nav_item(ui, "◈", "Home", self.screen == Screen::Home, t) {
                            self.screen = Screen::Home;
                        }
                        if nav_item(ui, "→", "Send money", self.screen == Screen::Send, t) {
                            self.screen = Screen::Send;
                            self.amount.clear();
                            self.counterparty.clear();
                        }
                        if nav_item(ui, "⌂", "Cash out", self.screen == Screen::CashOut, t) {
                            self.screen = Screen::CashOut;
                            self.amount.clear();
                            self.counterparty.clear();
                        }
                        if nav_item(ui, "≡", "History", self.screen == Screen::History, t) {
                            self.screen = Screen::History;
                        }
                    } else {
                        ui.label(
                            RichText::new("Sign in to continue")
                                .size(13.0)
                                .color(t.text_dim),
                        );
                    }

                    ui.add_space(20.0);
                    ui.separator();
                    ui.add_space(8.0);
                    let is_sim = self.backend.name() == "simulated";
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            if is_sim { t.positive } else { t.warning },
                            RichText::new("●").size(11.0),
                        );
                        ui.label(
                            RichText::new(format!("{} backend", self.backend.name())).size(12.0),
                        );
                    });
                    if *self.busy.lock().expect("busy lock") {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("working…").size(12.0).color(t.text_dim));
                        });
                    }
                    ui.add_space(12.0);
                    if secondary_button(ui, "Quit", t) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.bg_bottom))
            .show(ctx, |ui| {
                ui.add_space(10.0);
                self.status_banner(ui, t);
                match self.screen {
                    Screen::Login => self.login_ui(ui, t),
                    Screen::Home => self.home_ui(ui, t),
                    Screen::Send => self.transfer_ui(ui, t, TxForm::Send),
                    Screen::CashOut => self.transfer_ui(ui, t, TxForm::CashOut),
                    Screen::History => self.history_ui(ui, t),
                }
            });
    }

    fn status_banner(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let Some((msg, ok)) = self.status.clone() else {
            return;
        };
        let color = if ok { t.positive } else { t.negative };
        egui::Frame::new()
            .fill(color.gamma_multiply(0.16))
            .stroke(egui::Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(color, if ok { "✓" } else { "!" });
                    ui.label(RichText::new(msg).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("dismiss").clicked() {
                            self.status = None;
                        }
                    });
                });
            });
        ui.add_space(10.0);
    }

    fn login_ui(&mut self, ui: &mut egui::Ui, t: &Theme) {
        ui.vertical_centered(|ui| {
            ui.add_space(70.0);
            t.frame().show(ui, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new("bKash").size(30.0).color(t.accent).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new("native Rust client · runs on your PC")
                        .size(13.0)
                        .color(t.text_dim),
                );
                ui.add_space(24.0);

                if self.challenge.is_none() {
                    section_label(ui, "mobile number", t);
                    ui.add_space(6.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.mobile)
                            .hint_text("01XXXXXXXXX")
                            .desired_width(380.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(20.0);
                    let enabled = self.mobile.trim().len() >= 10;
                    ui.add_enabled_ui(enabled, |ui| {
                        if primary_button(ui, "Send OTP", t) {
                            self.status = None;
                            self.submit(Job::RequestOtp {
                                mobile: self.mobile.trim().to_string(),
                            });
                        }
                    });
                } else {
                    let dest = self
                        .challenge
                        .as_ref()
                        .map(|c| c.masked_destination.clone())
                        .unwrap_or_default();
                    ui.label(RichText::new(format!("Code sent to {dest}")).size(15.0));
                    if let Some(code) = self.challenge.as_ref().and_then(|c| c.demo_code.clone()) {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("demo mode — your OTP is {code}"))
                                .size(14.0)
                                .color(t.warning),
                        );
                    }
                    ui.add_space(14.0);
                    section_label(ui, "6-digit code", t);
                    ui.add_space(6.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.otp)
                            .hint_text("••••••")
                            .desired_width(380.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(20.0);
                    ui.add_enabled_ui(self.otp.len() == 6, |ui| {
                        if primary_button(ui, "Verify", t) {
                            self.submit(Job::VerifyOtp {
                                mobile: self.mobile.trim().to_string(),
                                otp: self.otp.trim().to_string(),
                            });
                        }
                    });
                    ui.add_space(8.0);
                    if ui.link("use a different number").clicked() {
                        self.challenge = None;
                        self.otp.clear();
                    }
                }
            });
        });
    }

    fn home_ui(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let Some(session) = self.session.clone() else {
            return;
        };
        title(ui, &session.account.name, t);
        ui.label(
            RichText::new(&session.account.masked_mobile)
                .size(13.0)
                .color(t.text_dim),
        );
        ui.add_space(18.0);

        t.frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "available balance", t);
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("Tk {}", session.account.balance.0))
                    .size(42.0)
                    .color(t.text)
                    .strong(),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if primary_button(ui, "Send money", t) {
                    self.screen = Screen::Send;
                    self.amount.clear();
                    self.counterparty.clear();
                }
                if secondary_button(ui, "Cash out", t) {
                    self.screen = Screen::CashOut;
                    self.amount.clear();
                    self.counterparty.clear();
                }
                if secondary_button(ui, "Refresh", t) {
                    self.submit(Job::Refresh);
                }
            });
        });

        ui.add_space(18.0);
        section_label(ui, "recent activity", t);
        ui.add_space(6.0);
        if self.history.is_empty() {
            ui.label(RichText::new("no transactions yet").color(t.text_dim));
        }
        for tx in self.history.iter().take(6) {
            self.tx_row(ui, tx, t);
        }
    }

    fn tx_row(&self, ui: &mut egui::Ui, tx: &Transaction, t: &Theme) {
        let color = if tx.incoming { t.positive } else { t.negative };
        egui::Frame::new()
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0_f32, t.stroke))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{}{}",
                                if tx.incoming { "+" } else { "−" },
                                tx.amount.format()
                            ))
                            .size(16.0)
                            .color(color)
                            .strong(),
                        );
                        ui.label(
                            RichText::new(kind_label(tx.kind))
                                .size(12.0)
                                .color(t.text_dim),
                        );
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&tx.counterparty).size(14.0));
                        meta_label(ui, &tx.trx_id, t);
                    });
                });
            });
        ui.add_space(8.0);
    }

    fn transfer_ui(&mut self, ui: &mut egui::Ui, t: &Theme, form: TxForm) {
        title(
            ui,
            match form {
                TxForm::Send => "Send money",
                TxForm::CashOut => "Cash out",
            },
            t,
        );
        ui.add_space(14.0);
        t.frame().show(ui, |ui| {
            ui.set_width(460.0);
            section_label(
                ui,
                match form {
                    TxForm::Send => "recipient mobile",
                    TxForm::CashOut => "agent mobile",
                },
                t,
            );
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.counterparty)
                    .hint_text("01XXXXXXXXX")
                    .desired_width(420.0)
                    .font(egui::TextStyle::Monospace),
            );
            ui.add_space(16.0);
            section_label(ui, "amount", t);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tk").size(16.0).color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut self.amount)
                        .hint_text("0")
                        .desired_width(220.0)
                        .font(egui::TextStyle::Monospace),
                );
                if ui.small_button("max").clicked() {
                    if let Some(s) = &self.session {
                        self.amount = s.account.balance.0.to_string();
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "single-transfer limit {}",
                    Bdt::SINGLE_TX_LIMIT.format()
                ))
                .size(12.0)
                .color(t.text_dim),
            );
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if primary_button(ui, "Submit", t) {
                    match self.parse_amount() {
                        Ok(amount) => {
                            let target = self.counterparty.trim().to_string();
                            self.status = None;
                            self.submit(match form {
                                TxForm::Send => Job::Send { to: target, amount },
                                TxForm::CashOut => Job::CashOut {
                                    agent: target,
                                    amount,
                                },
                            });
                            self.screen = Screen::Home;
                        }
                        Err(e) => self.status = Some((e.to_string(), false)),
                    }
                }
                if secondary_button(ui, "Cancel", t) {
                    self.screen = Screen::Home;
                }
            });
        });
    }

    fn history_ui(&mut self, ui: &mut egui::Ui, t: &Theme) {
        title(ui, "History", t);
        ui.add_space(10.0);
        if self.history.is_empty() {
            ui.label(RichText::new("no transactions yet").color(t.text_dim));
        }
        for tx in self.history.clone() {
            self.tx_row(ui, &tx, t);
        }
    }
}

#[derive(Clone, Copy)]
enum TxForm {
    Send,
    CashOut,
}

fn kind_label(kind: TxKind) -> &'static str {
    match kind {
        TxKind::SendMoney => "Send money",
        TxKind::CashOut => "Cash out",
        TxKind::TopUp => "Top up",
        TxKind::Payment => "Payment",
        TxKind::Unknown => "Transaction",
    }
}

/// Run the app. Blocks until the window closes.
pub fn run(backend: Arc<dyn Backend>) -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 740.0])
            .with_title("bKash — native Rust client"),
        ..Default::default()
    };
    eframe::run_native(
        "bKash native",
        native_options,
        Box::new(move |_cc| Ok(Box::new(BkashApp::new(backend)))),
    )
}

/// Convenience: the offline demo backend.
pub fn simulated() -> Arc<dyn Backend> {
    Arc::new(SimulatedBackend::new())
}
