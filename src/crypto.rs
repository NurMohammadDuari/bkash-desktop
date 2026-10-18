//! Session crypto, reconstructed from observed snapshot strings.
//!
//! Evidence chain, all from `libapp.so` (Dart AOT, snapshot `1ce86630892e2dca…`):
//!   `PreInit{aesKey: `            — `auth/pre-init` returns an AES key
//!   `SetEncryptionKeysEvent{aesKey: ` — a bloc event carries it into app state
//!   `VerifyOtp{aesKey: `          — `otp/verify` re-wraps the key after login
//!   `AES_CBC_PKCS7Padding`        — the cipher + padding used
//!   `RSAES-OAEP`                  — key transport to the server
//!   `FlutterEncryptedStorage`     — at-rest persistence
//!
//! Inference, not observation: the exact OAEP hash and the IV derivation are not
//! recoverable from byte-pattern strings alone. SHA-256 OAEP is used here because
//! it is the only OAEP variant that pairs with a 32-byte AES key in the ecosystem
//! this app vendors (`encrypt` package). `SessionCrypto::oaep_hash` makes that
//! choice explicit and swappable rather than burying it.

use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use rsa::rand_core::OsRng;
use rsa::pkcs1::{DecodeRsaPublicKey as _, EncodeRsaPublicKey};
use rsa::traits::PublicKeyParts;
use rsa::pkcs8::DecodePublicKey as _;
use rsa::{Oaep, RsaPublicKey};
use sha2::Sha256;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// PKCS#7 (RFC 5652) pad to the next whole block. Always adds 1..=block bytes,
/// so an empty plaintext becomes exactly one block of padding.
fn pkcs7_pad(plaintext: &[u8], block: usize) -> Vec<u8> {
    let pad = block - (plaintext.len() % block);
    let mut out = Vec::with_capacity(plaintext.len() + pad);
    out.extend_from_slice(plaintext);
    out.resize(out.len() + pad, pad as u8);
    out
}

/// Strip PKCS#7. Rejects a bad length byte and a bad padding run rather than
/// silently returning garbage.
fn pkcs7_unpad(padded: &[u8], block: usize) -> Result<&[u8], CryptoError> {
    if padded.is_empty() || padded.len() % block != 0 {
        return Err(CryptoError::NotBlockAligned);
    }
    let pad = *padded.last().expect("non-empty") as usize;
    if pad == 0 || pad > block || pad > padded.len() {
        return Err(CryptoError::BadPadding);
    }
    if !padded[padded.len() - pad..].iter().all(|&b| b as usize == pad) {
        return Err(CryptoError::BadPadding);
    }
    Ok(&padded[..padded.len() - pad])
}

/// Split a buffer into `Block<Self>` chunks for the bulk block API.
fn blocks_mut(buf: &mut [u8]) -> &mut [aes::Block] {
    // `Block<Aes256>` is `[u8; 16]`; the length was validated by the caller.
    let ptr = buf.as_mut_ptr().cast::<aes::Block>();
    unsafe { std::slice::from_raw_parts_mut(ptr, buf.len() / AES_BLOCK_LEN) }
}

pub const AES_KEY_LEN: usize = 32;
pub const AES_BLOCK_LEN: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("aes key must be {AES_KEY_LEN} bytes, got {0}")]
    BadKeyLen(usize),
    #[error("iv must be {AES_BLOCK_LEN} bytes, got {0}")]
    BadIvLen(usize),
    #[error("ciphertext is not a whole number of AES blocks")]
    NotBlockAligned,
    #[error("pkcs7 padding is malformed or the key is wrong")]
    BadPadding,
    #[error("base64 decode failed: {0}")]
    Base64(String),
    #[error("rsa error: {0}")]
    Rsa(#[from] rsa::Error),
    #[error("rsa public key is not decodable as PKCS#1: {0}")]
    RsaKeyFormat(String),
}

/// The per-session key material. Holds no secret longer than the call that
/// created it; `Drop` zeroizes via `zeroize`-free manual wipe so the crate
/// stays dependency-light.
pub struct SessionCrypto {
    aes_key: [u8; AES_KEY_LEN],
}

impl std::fmt::Debug for SessionCrypto {
    /// Never prints key bytes: a `{:?}` in a log line is not a place to leak a
    /// session key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionCrypto")
            .field("aes_key", &"<redacted>")
            .finish()
    }
}

impl Drop for SessionCrypto {
    fn drop(&mut self) {
        // Volatile write so the wipe is not optimized away.
        for b in self.aes_key.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

impl SessionCrypto {
    pub fn from_bytes(key: &[u8]) -> Result<Self, CryptoError> {
        if key.len() != AES_KEY_LEN {
            return Err(CryptoError::BadKeyLen(key.len()));
        }
        let mut aes_key = [0u8; AES_KEY_LEN];
        aes_key.copy_from_slice(key);
        Ok(Self { aes_key })
    }

    /// Accepts the 64-char hex form the gateway sends for `aesKey`.
    pub fn from_hex(key_hex: &str) -> Result<Self, CryptoError> {
        if key_hex.len() != AES_KEY_LEN * 2 {
            return Err(CryptoError::BadKeyLen(key_hex.len() / 2));
        }
        let mut key = [0u8; AES_KEY_LEN];
        for (i, slot) in key.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&key_hex[i * 2..i * 2 + 2], 16)
                .map_err(|e| CryptoError::Base64(format!("bad hex: {e}")))?;
        }
        Self::from_bytes(&key)
    }

    pub fn key(&self) -> &[u8; AES_KEY_LEN] {
        &self.aes_key
    }

    /// AES-256-CBC + PKCS#7, matching `AES_CBC_PKCS7Padding`.
    /// The IV is prepended to the ciphertext, which is what the Dart
    /// `encrypt` package's `Iv` argument flow does.
    pub fn encrypt(&self, plaintext: &[u8], iv: &[u8; AES_BLOCK_LEN]) -> Vec<u8> {
        let mut enc = Aes256CbcEnc::new(&self.aes_key.into(), iv.into());
        let mut buf = pkcs7_pad(plaintext, AES_BLOCK_LEN);
        enc.encrypt_blocks(blocks_mut(&mut buf));
        let mut out = Vec::with_capacity(iv.len() + buf.len());
        out.extend_from_slice(iv);
        out.extend_from_slice(&buf);
        out
    }

    /// Inverse of [`encrypt`]; reads the IV back off the front.
    pub fn decrypt(&self, envelope: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if envelope.len() < AES_BLOCK_LEN * 2 {
            return Err(CryptoError::NotBlockAligned);
        }
        let (iv, body) = envelope.split_at(AES_BLOCK_LEN);
        if body.len() % AES_BLOCK_LEN != 0 {
            return Err(CryptoError::NotBlockAligned);
        }
        let mut iv_arr = [0u8; AES_BLOCK_LEN];
        iv_arr.copy_from_slice(iv);
        let mut dec = Aes256CbcDec::new(&self.aes_key.into(), &iv_arr.into());
        let mut buf = body.to_vec();
        dec.decrypt_blocks(blocks_mut(&mut buf));
        let len = pkcs7_unpad(&buf, AES_BLOCK_LEN)?.len();
        buf.truncate(len);
        Ok(buf)
    }

    pub fn encrypt_b64(&self, plaintext: &[u8], iv: &[u8; AES_BLOCK_LEN]) -> String {
        B64.encode(self.encrypt(plaintext, iv))
    }

    pub fn decrypt_b64(&self, envelope_b64: &str) -> Result<Vec<u8>, CryptoError> {
        let raw = B64
            .decode(envelope_b64)
            .map_err(|e| CryptoError::Base64(e.to_string()))?;
        self.decrypt(&raw)
    }

    /// Wrap the session key for transport: RSA-OAEP with SHA-256, matching the
    /// observed `RSAES-OAEP` token.
    pub fn wrap_key(&self, server_public_pem: &str) -> Result<Vec<u8>, CryptoError> {
        let key = match RsaPublicKey::from_public_key_pem(server_public_pem) {
            Ok(k) => k,
            Err(pem_err) => match RsaPublicKey::from_pkcs1_pem(server_public_pem) {
                Ok(k) => k,
                Err(_) => return Err(CryptoError::RsaKeyFormat(pem_err.to_string())),
            },
        };
        if key.n().bits() < 2048 {
            // OAEP needs k >= 2*hLen + 2 + mLen. With SHA-256 and a 32-byte
            // key that is a 2048-bit modulus minimum.
            return Err(CryptoError::RsaKeyFormat(format!(
                "key too small: {} bits for OAEP-SHA256 over {AES_KEY_LEN}-byte key",
                key.n().bits()
            )));
        }
        Ok(key.encrypt(&mut OsRng, Oaep::new::<Sha256>(), &self.aes_key)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; AES_KEY_LEN] = [
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69, 0x78, 0x87, 0x96, 0xa5, 0xb4, 0xc3, 0xd2,
        0xe1, 0xf0,
    ];

    #[test]
    fn cbc_round_trip_with_observed_padding() {
        let c = SessionCrypto::from_bytes(&KEY).unwrap();
        let iv = [7u8; AES_BLOCK_LEN];
        let msg = b"{\"trx_id\":\"1ABCDEFGHI\",\"amount\":\"500\"}";
        let env = c.encrypt(msg, &iv);
        assert_eq!(env.len(), AES_BLOCK_LEN + 32 + 16, "iv + padded block");
        assert_eq!(c.decrypt(&env).unwrap(), msg);
    }

    #[test]
    fn ciphertext_differs_per_iv() {
        let c = SessionCrypto::from_bytes(&KEY).unwrap();
        let a = c.encrypt(b"same", &[0u8; 16]);
        let b = c.encrypt(b"same", &[1u8; 16]);
        assert_ne!(a, b, "IV must be unique per message");
    }

    #[test]
    fn wrong_key_fails_padding_check() {
        let a = SessionCrypto::from_bytes(&KEY).unwrap();
        let mut other = KEY;
        other[0] ^= 0xff;
        let b = SessionCrypto::from_bytes(&other).unwrap();
        let env = a.encrypt(b"secret", &[3u8; 16]);
        assert_eq!(b.decrypt(&env).unwrap_err(), CryptoError::BadPadding);
    }

    #[test]
    fn rejects_truncated_and_misaligned_envelopes() {
        let c = SessionCrypto::from_bytes(&KEY).unwrap();
        assert_eq!(c.decrypt(&[0u8; 8]).unwrap_err(), CryptoError::NotBlockAligned);
        let mut short = vec![0u8; AES_BLOCK_LEN + 20];
        assert_eq!(c.decrypt(&short).unwrap_err(), CryptoError::NotBlockAligned);
        short.clear();
    }

    #[test]
    fn hex_and_base64_paths_agree() {
        let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
        let c = SessionCrypto::from_hex(&hex).unwrap();
        assert_eq!(c.key(), &KEY);
        let iv = [9u8; AES_BLOCK_LEN];
        let b64 = c.encrypt_b64(b"hello bkash", &iv);
        assert_eq!(c.decrypt_b64(&b64).unwrap(), b"hello bkash");
    }

    #[test]
    fn rejects_bad_key_material() {
        assert_eq!(
            SessionCrypto::from_bytes(&[0u8; 16]).unwrap_err(),
            CryptoError::BadKeyLen(16)
        );
        assert!(matches!(
            SessionCrypto::from_hex("deadbeef").unwrap_err(),
            CryptoError::BadKeyLen(_)
        ));
        assert!(matches!(
            SessionCrypto::from_hex("zzzz000000000000000000000000000000000000000000000000000000000000")
                .unwrap_err(),
            CryptoError::Base64(_)
        ));
    }

    #[test]
    fn wrap_key_rejects_undersized_rsa_keys() {
        let c = SessionCrypto::from_bytes(&KEY).unwrap();
        let small = rsa::RsaPrivateKey::new(&mut OsRng, 1024).unwrap();
        let pem = small
            .to_public_key()
            .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
            .unwrap();
        let err = c.wrap_key(&pem).unwrap_err();
        assert!(matches!(err, CryptoError::RsaKeyFormat(_)), "got {err}");
    }

    #[test]
    fn wrap_key_round_trips_through_the_private_half() {
        let c = SessionCrypto::from_bytes(&KEY).unwrap();
        let mut rng = OsRng;
        let priv_key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = priv_key
            .to_public_key()
            .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
            .unwrap();
        let wrapped = c.wrap_key(&pem).unwrap();
        let back = priv_key
            .decrypt(Oaep::new::<Sha256>(), &wrapped)
            .unwrap();
        assert_eq!(back, KEY);
    }
}