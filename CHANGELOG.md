# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows [SemVer](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-10-18

### Added
- `bkanalyze`: dependency-free ELF64 reader that locates and decodes the four
  Dart AOT snapshot sections, validated against Ghidra's independent read.
- `bkanalyze --features`: classifies recovered identifiers into screens, blocs,
  bloc states/events, i18n keys, repositories and vendored packages.
- `bknative`: CLI over the recovered gateway surface, with a dry-run default.
- `bkash_desktop`: native egui GUI with login, balance, send money, cash out
  and history, running entirely offline against a simulated backend.
- `src/crypto.rs`: AES-256-CBC/PKCS#7 and RSA-OAEP-SHA256, reconstructed from
  the snapshot's crypto markers.
- Theme system with sidebar navigation, card surfaces and a dark palette.

### Known gaps
- Live authenticated sessions are not implemented. The request and response
  schema for `auth/pre-init` and `otp/verify` is inferred, not observed.
- Typed Dart class recovery from the snapshot is not implemented; the feature
  inventory is naming-convention inference over string constants.
