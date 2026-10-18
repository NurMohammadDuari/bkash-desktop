# Architecture

## Layering

```
                    ┌──────────────────────────────┐
                    │        src/app/ui.rs         │  egui shell
                    │  sidebar · cards · state    │
                    └───────────────┬──────────────┘
                                    │ trait Backend
                    ┌───────────────▼──────────────┐
                    │      src/app/backend.rs     │  seam
                    │  SimulatedBackend│LiveBackend│
                    └───────────────┬──────────────┘
                                    │ trait Transport
                    ┌───────────────▼──────────────┐
                    │  src/app/http.rs             │  the only
                    │  reqwest blocking + rustls   │  socket code
                    └──────────────────────────────┘
```

Two traits carry the whole design. The UI never imports `reqwest`; the backend
never knows about egui. That is what makes all 48 tests runnable offline.

## Modules

| Module | Lines | Responsibility |
|--------|-------|----------------|
| `src/elf.rs` | ~380 | ELF64-LE reader: `.dynsym`/`.dynstr`, Dart snapshot sections and headers |
| `src/strings.rs` | ~113 | Printable-run extraction, URL harvesting with explicit lower-bound semantics |
| `src/features.rs` | ~190 | Classifies recovered identifiers into architecture buckets |
| `src/crypto.rs` | ~330 | AES-256-CBC/PKCS#7, RSA-OAEP-SHA256, key hygiene |
| `src/domain.rs` | ~170 | Recovered gateways, endpoints, `trx_id` and mobile validation |
| `src/gateway.rs` | ~310 | Envelope types, request builder, `Transport` seam |
| `src/app/model.rs` | ~150 | `Bdt`, `Account`, `Transaction`, deterministic `trx_id` generation |
| `src/app/backend.rs` | ~300 | Backend trait, simulated + live implementations |
| `src/app/theme.rs` | ~200 | Palette, type scale, card/sidebar surfaces |
| `src/app/ui.rs` | ~450 | The GUI |

## Threading

`BkashApp::new` spawns exactly one worker thread. The UI sends `Job` values
over a channel; the worker owns the backend handle and returns `Reply` values
on another channel. The UI drains replies at the top of every frame.

This exists because a live gateway call can block for seconds. Blocking the
egui frame loop would freeze the window.

`Arc::clone` is used for every handle crossing into the thread; the session is
shared through `Arc<Mutex<Option<Session>>>` so transfer jobs carry the real
session rather than a probe.

## Money handling

`Bdt` is a newtype over `i64`. No floats anywhere. Every operation is
`checked_add`/`checked_sub` and returns `Option` — the UI surfaces a real error
instead of wrapping around.

## Crypto hygiene

* `SessionCrypto::drop` wipes the key with `write_volatile`.
* `SessionCrypto`'s `Debug` prints `<redacted>`.
* The IV is prepended to the ciphertext envelope so each message carries its own.
