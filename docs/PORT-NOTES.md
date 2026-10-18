# Port status — what is and is not ported

The honest version.

## Ported and verified

* **Dart AOT snapshot decoding.** `bkanalyze` locates the four snapshot
  sections, decodes the header, and recovers the snapshot hash. Cross-validated
  against Ghidra. 30 MB in 0.37 s.
* **Crypto.** AES-256-CBC with PKCS#7, IV prepended to the envelope;
  RSA-OAEP-SHA256 key wrapping. Eight unit tests including a wrap→unwrap
  round-trip and wrong-key padding rejection.
* **API surface.** Seven gateways and 33 endpoints, extracted from the string
  pool and compiled into `src/domain.rs`.
* **Architecture inventory.** 119 screens, 564 bloc states/events, 742 i18n
  keys, recovered automatically to JSON.
* **A working desktop client.** Login, balance, send money, cash out, history.
  Runs natively on X11 with no Android involved. Verified on screen.

## Not ported

* **119 screens.** Recovered as *names*. Zero of them are implemented. Their
  layout, copy and behaviour would have to be rebuilt by hand from the
  inventory and from observing the real app.
* **Typed Dart classes.** The inventory is naming-convention inference over
  string constants, not deserialized cluster semantics. A wrong item can be a
  real class, or a string that happens to look like one.
* **Live authentication.** The `--live` backend refuses to create a session.

## The signing problem

The two security libraries were decoded with Ghidra and change the picture:

* **`libsecurity_lite.so`** is Alipay **SecurityGuard Lite**, exposing
  `signRequest`, `encrypt`, `decrypt` and `decryptWithSecurityInfo` over JNI.
  Sensitive requests are signed by proprietary native code.
* **`libtoolChecker.so`** is the open-source **RootBeer** root detector.

A faithful interoperable client would need to produce signatures that
SecurityGuard's *server* accepts. The signing scheme is closed-source Alipay
code, and its verification counterpart lives on their servers — not in the APK.
So this is not an inference gap that a capture can close. It is a cryptographic
dependency on a third party.

This is the concrete reason the live backend refuses to create a session, and
it is the correct place for this project to stop.

## Why live auth is not implemented

The blocker is not the crypto — that is implemented. It is that **no request or
response body was ever observed**.

`auth/pre-init` and `otp/verify` are known only as endpoint-name strings plus
fragments like `PreInit{aesKey:`. The field names, types, headers and token
format were inferred. Shipping a login built on inferred schemas would either
fail at the server or, worse, appear to succeed while being wrong.

There is also the OAEP digest and IV derivation — both inferred. The snapshot
names the padding (`RSAES-OAEP`) but not the hash.

To close this gap you need one real capture from a real device on a real
account: a HAR from a proxy, or Frida hooks on the app's own crypto. Both are
legitimate on your own account and device. Hand that to this project and the
inferences become observations.

## What this project will not do

Bypass device attestation, root detection, or emulator detection. That is a
security control on a financial application, and defeating it is not
interoperability.
