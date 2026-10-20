//! The metagame's cryptography, byte-compatible with the Node metagame (MIGRATION_PLAN.md M20-M23):
//! tokens and recovery blobs it issued keep working after the cutover.
//!
//! - Session tokens: RS256 JWTs with issuer and audience `undaunted-metagame`, 24 hours, no clock
//!   leeway (`controllers/auth.ts`). Only RS256 is accepted, so `alg: none` and HS256 signed with
//!   the public key are refused.
//! - API keys are stored as the hex SHA-256 of their UTF-8 text and compared in constant time.
//! - Account key recovery (`controllers/accountrecovery.ts`): AES-256-GCM under
//!   SHA-256(`dauntless-account-recovery-v1\0` + the signing key's base64), the user id as associated
//!   data, stored as base64 of nonce (12 bytes) + ciphertext + tag (16 bytes).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const TOKEN_ISSUER: &str = "undaunted-metagame";
pub const TOKEN_LIFETIME_SECS: u64 = 24 * 60 * 60;
const RECOVERY_CONTEXT: &str = "dauntless-account-recovery-v1\0";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the token signing keys can't be read: {0}")]
    Keys(String),
    #[error("the token is not valid: {0}")]
    InvalidToken(String),
    #[error("the recovery blob can't be decrypted")]
    Recovery,
}

/// The claims the metagame signs, in the order Node's jsonwebtoken writes them.
#[derive(Debug, Serialize)]
struct SignedClaims<'a> {
    #[serde(rename = "userId")]
    user_id: &'a str,
    epoch: i64,
    iat: u64,
    exp: u64,
    aud: &'static str,
    iss: &'static str,
}

/// What a verified token says. `epoch` is missing in tokens issued before key rotation existed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenClaims {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub epoch: Option<i64>,
}

/// The signing key pair, from `AUTH_SIGNING_PRIVKEY_B64` and `AUTH_SIGNING_PUBKEY_B64` (base64 of PEM).
pub struct TokenKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
}

impl TokenKeys {
    pub fn from_base64_pem(private_b64: &str, public_b64: &str) -> Result<Self, CryptoError> {
        let pem = |b64: &str| BASE64.decode(b64.trim()).map_err(|e| CryptoError::Keys(e.to_string()));
        let encoding = EncodingKey::from_rsa_pem(&pem(private_b64)?).map_err(|e| CryptoError::Keys(e.to_string()))?;
        let decoding = DecodingKey::from_rsa_pem(&pem(public_b64)?).map_err(|e| CryptoError::Keys(e.to_string()))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[TOKEN_ISSUER]);
        validation.set_audience(&[TOKEN_ISSUER]);
        validation.leeway = 0;
        validation.validate_nbf = true;
        Ok(Self {
            encoding,
            decoding,
            validation,
        })
    }

    /// A session token for `user_id` at key epoch `epoch`, issued at `now` (seconds since 1970).
    pub fn sign(&self, user_id: &str, epoch: i64, now: u64) -> Result<String, CryptoError> {
        let claims = SignedClaims {
            user_id,
            epoch,
            iat: now,
            exp: now + TOKEN_LIFETIME_SECS,
            aud: TOKEN_ISSUER,
            iss: TOKEN_ISSUER,
        };
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.encoding)
            .map_err(|e| CryptoError::Keys(e.to_string()))
    }

    /// Checks signature, algorithm, issuer, audience and expiry. Epoch and ban checks are the caller's.
    pub fn verify(&self, token: &str) -> Result<TokenClaims, CryptoError> {
        jsonwebtoken::decode::<TokenClaims>(token, &self.decoding, &self.validation)
            .map(|data| data.claims)
            .map_err(|e| CryptoError::InvalidToken(e.to_string()))
    }
}

/// Lower-case hex SHA-256 of `text`: how API keys and key fingerprints are stored.
pub fn sha256_hex(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

/// Whether two stored hex hashes are equal, in time that doesn't depend on where they differ.
pub fn hashes_equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `n` random bytes as lower-case hex, like `randomBytes(n).toString("hex")`.
pub fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill(&mut bytes[..]);
    hex(&bytes)
}

/// A random version-4 UUID in lower case, like `randomUUID()`.
pub fn random_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A uniformly random integer in `0..end`, like `randomInt(0, end)`.
pub fn random_below(end: usize) -> usize {
    rand::rng().random_range(0..end)
}

fn recovery_cipher(signing_key_b64: &str) -> Result<Aes256Gcm, CryptoError> {
    let key = Sha256::digest(format!("{RECOVERY_CONTEXT}{signing_key_b64}").as_bytes());
    Aes256Gcm::new_from_slice(&key).map_err(|_| CryptoError::Recovery)
}

/// Encrypts an account key for recovery, with a fresh nonce.
pub fn seal_recovery(signing_key_b64: &str, user_id: &str, account_key: &str) -> Result<String, CryptoError> {
    let mut nonce = [0u8; 12];
    rand::rng().fill(&mut nonce);
    let sealed = recovery_cipher(signing_key_b64)?
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: account_key.as_bytes(),
                aad: user_id.as_bytes(),
            },
        )
        .map_err(|_| CryptoError::Recovery)?;
    let mut blob = nonce.to_vec();
    blob.extend_from_slice(&sealed);
    Ok(BASE64.encode(blob))
}

/// Decrypts a recovery blob; fails if it was made for another user id or key, or was changed.
pub fn open_recovery(signing_key_b64: &str, user_id: &str, blob: &str) -> Result<String, CryptoError> {
    let data = BASE64.decode(blob).map_err(|_| CryptoError::Recovery)?;
    if data.len() < 12 + 16 {
        return Err(CryptoError::Recovery);
    }
    let (nonce, sealed) = data.split_at(12);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| CryptoError::Recovery)?;
    let plain = recovery_cipher(signing_key_b64)?
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: sealed,
                aad: user_id.as_bytes(),
            },
        )
        .map_err(|_| CryptoError::Recovery)?;
    String::from_utf8(plain).map_err(|_| CryptoError::Recovery)
}
