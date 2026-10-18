# Contributing

Thanks for looking at this. It is a research project; contributions are welcome.

## Getting set up

```bash
cargo build
cargo test        # 48 tests, all offline, no network required
cargo run --bin bkash_desktop
```

## Where to help

| Area | File | Notes |
|------|------|-------|
| AOT snapshot decoding | `src/elf.rs` | SDK-matched typed cluster decoding is the big gap |
| API schema | `src/domain.rs` | **Everything here is inferred.** Observed values beat inferred ones — bring captures. |
| Crypto | `src/crypto.rs` | OAEP digest and IV derivation are inferred, not observed |
| UI | `src/app/ui.rs`, `src/app/theme.rs` | egui; keep the design system in `theme.rs` |
| Docs | `docs/` | Corrections to observed-vs-inferred are the most valuable PRs |

## The one rule

**Never mark something as observed unless you observed it.** If a value came
from a naming convention or a plausible guess, say so in the comment. The
project's value is that its evidence trail is honest.

## Style

* `cargo clippy --all-targets` clean.
* New behaviour gets a test. Tests must not touch the network.
* Comments explain *why*, not *what*.

## Commit messages

Conventional-ish: `feat:`, `fix:`, `docs:`, `test:`.
