<div align="center">

# bKash Desktop

**A native Rust reimplementation of the bKash Android client that runs on your PC.**

No emulator. No Android. No APK at runtime. Just a Rust binary with a native GUI.

[![CI](https://github.com/OWNER/bkash-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/bkash-desktop/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-000?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

---

## What this is

bKash ships as a **Flutter app**. Its entire UI and business logic is compiled
to ARM64 machine code inside a 30 MB Dart AOT snapshot (`libapp.so`), wrapped
in a three-file Java shell. There is no Kotlin source to port — the logic is
machine code.

This project takes what *is* recoverable from the shipped artifact and rebuilds
it natively in Rust:

| Layer | Status |
|---|---|
| Dart AOT snapshot decoder (ELF64 + snapshot header) | **Done** — cross-validated against Ghidra |
| Crypto scheme (AES-256-CBC/PKCS#7, RSA-OAEP-SHA256) | **Done** — 8 unit tests |
| API surface recovery (7 gateways, 33 endpoints) | **Done** — extracted from the string pool |
| Architecture recovery (119 screens, 564 bloc states, 742 i18n keys) | **Done** — automated inventory |
| Native desktop GUI (egui) | **Done** — runs, verified on screen |
| Live authenticated session | **Not implemented** — requires a captured real handshake |

## Quick start

```bash
git clone https://github.com/OWNER/bkash-desktop
cd bkash-desktop
cargo build --release
./target/release/bkash_desktop
```

A window opens. Enter any 11-digit Bangladeshi mobile number, press
**Send OTP**, and the demo OTP is shown on screen. Enter it, press **Verify**,
and you land on a working account screen with balance, send-money, cash-out and
history.

## The tools

Two binaries, both usable standalone:

```bash
# Decode the Dart AOT payload — 30 MB in 0.37s, pure Rust, no Java
bkanalyze libapp.so --urls --features --json features.json

# Show the recovered API surface
bknative endpoints
bknative envelope 01712345678 123456      # dry-run: prints what would be sent
```

## Why it is built this way

* **Zero Android dependencies at runtime.** `bkanalyze` is a pure Rust ELF
  reader — no JADX, no Ghidra, no Python.
* **Every constant is traceable.** Each gateway URL, endpoint and crypto marker
  in `src/domain.rs` and `src/crypto.rs` was recovered from the shipped
  artifact. Nothing is invented. Where something *is* inferred, the comment says
  so.
* **Testable without a network.** The backend is a trait. The simulated backend
  and all 48 tests run offline; the HTTP client is the only piece that knows
  about sockets.

## Documentation

* [Architecture](docs/ARCHITECTURE.md) — how every module fits together
* [Reverse-engineering notes](docs/ANALYSIS.md) — what the artifact contained
* [Port status](docs/PORT-NOTES.md) — what is ported and what is not, honestly

## Legal

This project contains **no bKash code or assets**. It is an independent
reimplementation produced by static analysis of a publicly obtainable APK, for
research and interoperability purposes. It is not affiliated with, endorsed by,
or connected to bKash or any related company. It ships with an offline
simulated backend by default and performs no live financial transactions.

See [SECURITY.md](SECURITY.md) and [LICENSE](LICENSE).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). PRs welcome — especially ones that
replace the inferred API schema with observed values.