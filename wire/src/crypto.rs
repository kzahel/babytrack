//! Version-1 cryptographic building blocks shared by native and web clients.
//!
//! Nonce allocation and persistence are owned by the batch/outbox layer.

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::cbor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidLabel,
    NoncanonicalCbor,
    InvalidPublicKey,
    InvalidSignature,
    EncryptionFailed,
    AuthenticationFailed,
}

/// `SHA256("babytrack/v1/" || label || 0x00 || bytes)`.
pub fn hash(label: &str, bytes: &[u8]) -> Result<[u8; 32], Error> {
    if label.is_empty() || !label.is_ascii() || label.as_bytes().contains(&0) {
        return Err(Error::InvalidLabel);
    }
    let mut digest = Sha256::new();
    digest.update(b"babytrack/v1/");
    digest.update(label.as_bytes());
    digest.update([0]);
    digest.update(bytes);
    Ok(digest.finalize().into())
}

pub fn signing_public_key(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// Sign the domain-separated digest of one canonical CBOR value (plain Ed25519).
pub fn sign_cbor(label: &str, canonical_value: &[u8], seed: &[u8; 32]) -> Result<[u8; 64], Error> {
    cbor::decode(canonical_value).map_err(|_| Error::NoncanonicalCbor)?;
    let digest = hash(label, canonical_value)?;
    Ok(SigningKey::from_bytes(seed).sign(&digest).to_bytes())
}

pub fn verify_cbor(
    label: &str,
    canonical_value: &[u8],
    public_key: &[u8; 32],
    signature: &[u8; 64],
) -> Result<(), Error> {
    cbor::decode(canonical_value).map_err(|_| Error::NoncanonicalCbor)?;
    let digest = hash(label, canonical_value)?;
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| Error::InvalidPublicKey)?;
    key.verify_strict(&digest, &Signature::from_bytes(signature))
        .map_err(|_| Error::InvalidSignature)
}

/// Encrypt with a supplied nonce. The caller must allocate a fresh random
/// nonce for each new ciphertext under this key; retries reuse stored bytes.
pub fn seal_with_nonce(
    key: &[u8; 32],
    nonce: &[u8; 24],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, Error> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::from(*nonce);
    cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::EncryptionFailed)
}

pub fn open(
    key: &[u8; 32],
    nonce: &[u8; 24],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, Error> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::from(*nonce);
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| Error::AuthenticationFailed)
}
