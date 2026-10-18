# Reverse-engineering notes

Artifact: `bKash_7.3.5_apkcube.apks` (APKCube repackage of the Play build).
Base APK SHA-256: `bad93da5f54834920dc9652854715115ce4313babc238c6bddf34be1c1ef7a55`.
`libapp.so` SHA-256: `0b1ef5c1f2dfd7579dd3e691a98e615232a8f1d55256e47c998b37a2c9864149`.

## Package

| Field | Value |
|---|---|
| Package | `com.bKash.customerapp` |
| Version | 7.3.5 (`versionCode 39008733`) |
| minSdk / targetSdk | 23 / 35 |
| Permissions | 30, including `CAMERA`, `READ_CONTACTS`, `NFC`, `READ_PHONE_STATE` |
| Classes | 26,460 |
| Resources | 2,141 |

## The key finding

Decompiling with JADX produced 26,234 Java files, and the entire application is:

```java
package com.bKash.customerapp;

public final class MainActivity extends FlutterFragmentActivity {
    @Override public void onCreate(@Nullable Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
    }
}
```

`com/bKash/customerapp/` contains three files: `MainActivity`,
`DataBinderMapperImpl`, `R`. **Every feature, screen and network call is
Flutter/Dart, compiled to ARM64 inside `libapp.so`.**

## Dart AOT snapshot

`libapp.so` is 30,409,648 bytes. It carries four exported snapshot sections:

| Symbol | File offset | Size |
|---|---|---|
| `_kDartVmSnapshotData` | `0x340` | 18,256 |
| `_kDartIsolateSnapshotData` | `0x4ac0` | 8,820,976 |
| `_kDartVmSnapshotInstructions` | `0x870000` | 92,480 |
| `_kDartIsolateSnapshotInstructions` | `0x886940` | 21,440,592 |

Header layout, read directly from the bytes at `0x4ac0`:

```
f5 f5 dc dc   magic  = 0xdcdcf5f5
f5 e7 64 00   length = 6,602,981
00 00 00 00   kind   = 0
03 00 00 00   version= 3
00 00 00 00   features
"1ce86630892e2dca9a85"
```

Snapshot hash: `1ce86630892e2dca9a85…`. These values match an independent Ghidra
read of the same file exactly.

## Ghidra's limits here

Ghidra 12.1.4 was driven through REA against `libapp.so`:

* `procedure_count: 0` across 21 MB of `.text`
* `string_count: 53` — all from `.dynstr` and `.shstrtab`
* Searching for `AES_CBC_PKCS7Padding` and `RSAES-OAEP` returned **no matches**

A stripped Dart AOT snapshot ships no symbols and a 62-byte `.eh_frame`, so
Ghidra has nothing to seed function discovery from. The Dart string constants
live inside one opaque `.rodata` blob rather than as discrete defined data.

This is why `src/elf.rs` exists: it recovers the sections, header and hash
directly, and harvests 43,955 printable runs — 230 absolute URLs — that Ghidra's
defined-data pass misses entirely.

## What the string pool revealed

Dart package: **`bkash_next`**.

Architecture, from `bkanalyze --features` over 3,987 identifier tokens:

| Bucket | Count |
|---|---|
| i18n keys | 742 |
| Bloc states and events | 564 |
| Screens / routes | 119 |
| Repositories / use cases | 5 |
| Vendored packages | 6 |

Crypto markers recovered:

```
AES_CBC_PKCS7Padding
RSAES-OAEP
FlutterEncryptedStorage
PreInit{aesKey:
SetEncryptionKeysEvent{aesKey:
VerifyOtp{aesKey:
```

Gateways recovered (7) — including `api.cde.capp.bka.sh`, `api.ims.bka.sh`,
`huawei.openfinapi.bka.sh` and `qr.bka.sh`. Relative endpoints recovered: 33,
including the `auth/*`, `otp/*` and twenty `ffs/loan/v2/*` paths.

## Native libraries

Fourteen ARM64 libraries ship in `split_config.arm64_v8a.apk`. Two are
security-relevant and were decoded with Ghidra:

### `libsecurity_lite.so` — Alipay SecurityGuard

SHA-256 `e6001d6f5463ec5f8f5a81fee35b67a378bcff8a7db1d0b5aa7a19fc8c794b5a`.
Stripped, 172 procedures and 154 strings recovered — unlike `libapp.so`, this
library has ordinary function structure.

Five JNI exports, all from `com.alipay.plus.security.lite.SecurityGuardLiteNative`:

```
Java_…_init
Java_…_signRequest
Java_…_encrypt
Java_…_decrypt
Java_…_decryptWithSecurityInfo
```

plus `parseAPKSign` and a bignum helper set (`bn_assign`, `bn_assign_2exp`,
`bn_assign_zero`).

**This is Alipay's anti-fraud and request-signing SDK.** Its presence is the
single most important finding for anyone attempting an interoperable client:
sensitive requests are signed by a proprietary native library. The signing
scheme is not recoverable from the shipped binary — it is closed-source Alipay
code, and its server-side component is not in the APK at all.

### `libtoolChecker.so` — RootBeer

SHA-256 `eeb2317e649e287b836b5e5c30cb07700f9eb900a53c297a3823592f15aa6352`.
27 procedures, 44 strings.

```
Java_com_scottyab_rootbeer_RootBeerNative_checkForRoot
Java_com_scottyab_rootbeer_RootBeerNative_setLogDebugMessages
_Z6existsPKc            (demangled: exists(char const*))
```

Log strings confirm the behaviour: `LOOKING FOR BINARY: %s PRESENT!!!` /
`Absent :(`. This is [RootBeer](https://github.com/scottyab/rootbeer), the
open-source root detector, built with NDK clang 18.0.1.

### The remaining libraries

`libflutter.so` and `libapp.so` (the Dart payload), `libbarhopper_v3.so` (QR
scanning), `libface_detector_v2_jni.so`, `libimage_processing_util_jni.so`,
`libmaplibre.so`, `librive_native.so`, `libtoyger.so` (Alipay face liveness),
`libgifimage.so`, `libimagepipeline.so`, `libdatastore_shared_counter.so`,
`libsurface_util_jni.so`.

## The two downloaded bundles are content-identical

`bKash_7.3.5_apkcube.apks` and `…_apkcube2.apks` have different SHA-256
digests but identical members — same file list, same per-member hashes. Only
ZIP container metadata differs. There is no second version to compare.

## Third-party components observed

`assets/hummer_task.json` routes eKYC to Alipay's Zoloz stack (`ZDoc`,
`ZFace`, `faceGuide`), with the document-select and face-guide web pages hosted
on `zc-face.assets.alipay.net`.

So three separate Alipay technologies are embedded: **Zoloz** for identity
verification, **SecurityGuard Lite** for request signing, and **Toyger** for
face liveness. Root detection is the third-party open-source RootBeer.

## Not done

* The APK was never executed.
* Signatures were never verified.
* No live traffic was captured, so every request and response **schema** in
  `src/domain.rs` is inferred from endpoint names and string fragments.
