//! RFC 9180 base-mode HPKE suite 0x0020/0x0001/0x0003.

use ::hpke::{
    Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable, aead::ChaCha20Poly1305,
    kdf::HkdfSha256, kem::X25519HkdfSha256, rand_core::CryptoRng, setup_receiver,
    setup_sender_with_rng,
};

type Kem = X25519HkdfSha256;
type Kdf = HkdfSha256;
type Aead = ChaCha20Poly1305;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidKey,
    InvalidInfo,
    SealFailed,
    OpenFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    pub enc: [u8; 32],
    pub ciphertext: Vec<u8>,
}

pub fn public_key_from_private(private_key: &[u8; 32]) -> Result<[u8; 32], Error> {
    let private =
        <Kem as KemTrait>::PrivateKey::from_bytes(private_key).map_err(|_| Error::InvalidKey)?;
    let public = Kem::sk_to_pk(&private).to_bytes();
    Ok(public
        .as_slice()
        .try_into()
        .expect("X25519 public key is 32 bytes"))
}

/// The caller supplies a cryptographically secure RNG. Each invocation uses
/// a fresh ephemeral key; never persist or reuse a seeded test RNG in a client.
pub fn seal_with_rng(
    recipient_public_key: &[u8; 32],
    info: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    rng: &mut impl CryptoRng,
) -> Result<Sealed, Error> {
    // The hpke crate panics when info.len() + 5 reaches 2^16 in base mode.
    if info.len() >= 65_531 {
        return Err(Error::InvalidInfo);
    }
    let recipient = <Kem as KemTrait>::PublicKey::from_bytes(recipient_public_key)
        .map_err(|_| Error::InvalidKey)?;
    let (enc, mut context) =
        setup_sender_with_rng::<Aead, Kdf, Kem>(&OpModeS::Base, &recipient, info, rng)
            .map_err(|_| Error::SealFailed)?;
    let ciphertext = context
        .seal(plaintext, aad)
        .map_err(|_| Error::SealFailed)?;
    let enc = enc.to_bytes();
    Ok(Sealed {
        enc: enc.as_slice().try_into().expect("X25519 enc is 32 bytes"),
        ciphertext,
    })
}

pub fn open(
    recipient_private_key: &[u8; 32],
    enc: &[u8; 32],
    info: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, Error> {
    if info.len() >= 65_531 {
        return Err(Error::InvalidInfo);
    }
    let private = <Kem as KemTrait>::PrivateKey::from_bytes(recipient_private_key)
        .map_err(|_| Error::InvalidKey)?;
    let encapsulated =
        <Kem as KemTrait>::EncappedKey::from_bytes(enc).map_err(|_| Error::InvalidKey)?;
    let mut context =
        setup_receiver::<Aead, Kdf, Kem>(&OpModeR::Base, &private, &encapsulated, info)
            .map_err(|_| Error::OpenFailed)?;
    context.open(ciphertext, aad).map_err(|_| Error::OpenFailed)
}
