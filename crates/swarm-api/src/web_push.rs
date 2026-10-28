//! Web Push, signed and encrypted here rather than by a library (ADR 0127).
//!
//! A push is a POST to the browser's push service carrying two things: a VAPID
//! token saying this Hive sent it (RFC 8292), and a body only the subscribed
//! browser can read (RFC 8291, in RFC 8188's aes128gcm encoding).
//! web-push-native did both on an elliptic-curve release whose bignum library
//! leaks timing (RUSTSEC-2026-0387), and had no release past it.

use std::time::Duration;

use aes_gcm::{Aes128Gcm, KeyInit, Nonce, aead::Aead};
use base64ct::{Base64UrlUnpadded, Encoding};
use hkdf::Hkdf;
use p256::{
    PublicKey, SecretKey,
    ecdsa::{Signature, SigningKey, signature::Signer},
    elliptic_curve::sec1::ToSec1Point,
};
use sha2::Sha256;

/// The record size a push body declares. One record carries the whole
/// message, which is far smaller.
const RECORD_SIZE: u32 = 4096;
/// The longest a VAPID token may be valid for (RFC 8292, section 2).
const MAX_VAPID_VALIDITY: Duration = Duration::from_secs(24 * 60 * 60);
/// How many random draws a new key may take before generation is called broken.
/// A draw fails only when it lands outside the curve's order, about once in
/// 2^32 draws.
const MAX_KEY_DRAWS: usize = 8;
/// The bytes of a P-256 private key, and of a subscription's auth secret.
const PRIVATE_KEY_BYTES: usize = 32;
const AUTH_SECRET_BYTES: usize = 16;
/// The record delimiter that marks the only record as the last (RFC 8188).
const LAST_RECORD: u8 = 2;
/// What AES-GCM adds to every record.
const TAG_BYTES: usize = 16;

/// This Hive's VAPID key: the identity push services see on every push.
pub struct VapidKey {
    signing: SigningKey,
}

impl VapidKey {
    /// # Errors
    /// Returns a failure to read the system's random source.
    pub fn generate() -> Result<Self, String> {
        Ok(Self {
            signing: SigningKey::from(random_secret()?),
        })
    }

    /// Restores a key from the 32 bytes `to_bytes` wrote, which are also the
    /// bytes web-push-native stored, so existing subscriptions keep working.
    ///
    /// # Errors
    /// Refuses anything that is not a P-256 private key.
    pub fn from_bytes(private_key: &[u8]) -> Result<Self, String> {
        if private_key.len() != PRIVATE_KEY_BYTES {
            return Err("stored notification key is not a P-256 private key".to_owned());
        }
        SigningKey::from_slice(private_key)
            .map(|signing| Self { signing })
            .map_err(|_| "stored notification key is not a P-256 private key".to_owned())
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.signing.to_bytes().to_vec()
    }

    /// The public key, uncompressed, as browsers subscribe with it.
    #[must_use]
    pub fn public_key(&self) -> Vec<u8> {
        self.signing
            .verifying_key()
            .to_sec1_point(false)
            .as_bytes()
            .to_vec()
    }

    /// The Authorization header for one push: a token for the push service's
    /// origin, valid for `valid_for` from `now` and never past a day, and the
    /// key that signed it.
    ///
    /// # Errors
    /// Refuses an endpoint with no origin to address the token to.
    pub fn authorization(
        &self,
        endpoint: &reqwest::Url,
        subject: &str,
        valid_for: Duration,
        now: i64,
    ) -> Result<String, String> {
        let origin = endpoint.origin();
        if !origin.is_tuple() {
            return Err("push endpoint has no origin".to_owned());
        }
        let valid_for = i64::try_from(valid_for.min(MAX_VAPID_VALIDITY).as_secs())
            .map_err(|_| "push validity does not fit a timestamp".to_owned())?;
        let claims = serde_json::json!({
            "aud": origin.ascii_serialization(),
            "exp": now.saturating_add(valid_for),
            "sub": subject,
        });
        let signed = format!(
            "{}.{}",
            Base64UrlUnpadded::encode_string(br#"{"typ":"JWT","alg":"ES256"}"#),
            Base64UrlUnpadded::encode_string(claims.to_string().as_bytes())
        );
        let signature: Signature = self.signing.sign(signed.as_bytes());
        Ok(format!(
            "vapid t={signed}.{}, k={}",
            Base64UrlUnpadded::encode_string(&signature.to_bytes()),
            Base64UrlUnpadded::encode_string(&self.public_key())
        ))
    }
}

/// Encrypts one push body so only the subscribed browser can read it, under a
/// fresh key and salt.
///
/// # Errors
/// Refuses an auth secret that is not 16 bytes and a message too long for one
/// record, and returns a failure to read the system's random source.
pub fn encrypt(plaintext: &[u8], subscriber: &PublicKey, auth: &[u8]) -> Result<Vec<u8>, String> {
    let mut salt = [0; 16];
    getrandom::fill(&mut salt).map_err(|error| error.to_string())?;
    encrypt_with(plaintext, subscriber, auth, &random_secret()?, salt)
}

/// RFC 8291, section 3.4, with the sender's key and the salt supplied, so the
/// published example can be reproduced exactly.
fn encrypt_with(
    plaintext: &[u8],
    subscriber: &PublicKey,
    auth: &[u8],
    sender: &SecretKey,
    salt: [u8; 16],
) -> Result<Vec<u8>, String> {
    if auth.len() != AUTH_SECRET_BYTES {
        return Err("subscription auth secret is not 16 bytes".to_owned());
    }
    let record_bytes = plaintext.len() + 1 + TAG_BYTES;
    if record_bytes > RECORD_SIZE as usize {
        return Err("push message does not fit one record".to_owned());
    }
    let sender_public = sender.public_key().to_sec1_point(false);
    let subscriber_public = subscriber.to_sec1_point(false);
    let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), subscriber.as_affine());

    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(subscriber_public.as_bytes());
    key_info.extend_from_slice(sender_public.as_bytes());
    let mut ikm = [0; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes().as_slice())
        .expand(&key_info, &mut ikm)
        .map_err(|_| "push key derivation failed".to_owned())?;
    let content = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut key = [0; 16];
    let mut nonce = [0; 12];
    content
        .expand(b"Content-Encoding: aes128gcm\0", &mut key)
        .and_then(|()| content.expand(b"Content-Encoding: nonce\0", &mut nonce))
        .map_err(|_| "push key derivation failed".to_owned())?;

    let mut record = Vec::with_capacity(record_bytes);
    record.extend_from_slice(plaintext);
    record.push(LAST_RECORD);
    let sealed = Aes128Gcm::new_from_slice(&key)
        .map_err(|_| "push content key is invalid".to_owned())?
        .encrypt(Nonce::from_slice(&nonce), record.as_slice())
        .map_err(|_| "push encryption failed".to_owned())?;

    let key_id = sender_public.as_bytes();
    let key_id_len =
        u8::try_from(key_id.len()).map_err(|_| "push key id is too long".to_owned())?;
    let mut body = Vec::with_capacity(salt.len() + 5 + key_id.len() + sealed.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(key_id_len);
    body.extend_from_slice(key_id);
    body.extend_from_slice(&sealed);
    Ok(body)
}

/// A new private key from the system's random source.
fn random_secret() -> Result<SecretKey, String> {
    for _ in 0..MAX_KEY_DRAWS {
        let mut bytes = [0; PRIVATE_KEY_BYTES];
        getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
        if let Ok(secret) = SecretKey::from_slice(&bytes) {
            return Ok(secret);
        }
    }
    Err("no valid P-256 key in eight random draws".to_owned())
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::{VerifyingKey, signature::Verifier};
    use serde_json::Value;

    use super::*;

    fn decoded(value: &str) -> Vec<u8> {
        Base64UrlUnpadded::decode_vec(value).unwrap()
    }

    /// RFC 8291 Appendix A, reproduced byte for byte: the example's keys and
    /// salt give the example's body. It also proves a 32-byte private key, the
    /// form web-push-native stored, derives the public key browsers know.
    ///
    /// ABLATION: drop the last-record delimiter, or swap the two keys in the
    /// key info, and the body no longer matches.
    #[test]
    fn the_rfc_8291_example_encrypts_to_its_published_body() {
        let sender =
            SecretKey::from_slice(&decoded("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        let sender_public = decoded(
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8",
        );
        assert_eq!(
            VapidKey::from_bytes(&sender.to_bytes())
                .unwrap()
                .public_key(),
            sender_public
        );
        let subscriber = PublicKey::from_sec1_bytes(&decoded(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        ))
        .unwrap();
        let salt: [u8; 16] = decoded("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();

        let body = encrypt_with(
            b"When I grow up, I want to be a watermelon",
            &subscriber,
            &decoded("BTBZMqHH6r4Tts7J_aSIgg"),
            &sender,
            salt,
        )
        .unwrap();

        let header = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
                      mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";
        let ciphertext = "8pfeW0KbunFT06SuDKoJH9Ql87S1QUrdirN6GcG7sFz1y1sqLgVi1VhjVkHsUoEs\
                          bI_0LpXMuGvnzQ";
        assert_eq!(body, [decoded(header), decoded(ciphertext)].concat());
    }

    #[test]
    fn encryption_refuses_a_short_secret_and_an_oversized_message() {
        let subscriber = SecretKey::from_slice(&[7; 32]).unwrap().public_key();
        assert!(encrypt(b"hello", &subscriber, &[0; 15]).is_err());
        assert!(encrypt(&[0; RECORD_SIZE as usize], &subscriber, &[0; 16]).is_err());
        let first = encrypt(b"hello", &subscriber, &[0; 16]).unwrap();
        let second = encrypt(b"hello", &subscriber, &[0; 16]).unwrap();
        // The body opens with the salt, then the record size and the sender's
        // key: each must be fresh, not only the whole.
        assert_ne!(first[..16], second[..16], "every push takes a fresh salt");
        assert_ne!(
            first[21..86],
            second[21..86],
            "every push takes a fresh key"
        );
    }

    /// The token names the push service's origin, expires within a day, and
    /// verifies under the key it carries; another key's signature does not.
    #[test]
    fn a_vapid_header_is_a_verifiable_token_for_the_push_origin() {
        let key = VapidKey::from_bytes(&[3; 32]).unwrap();
        let endpoint = reqwest::Url::parse("https://fcm.googleapis.com/wp/abc:123").unwrap();
        let header = key
            .authorization(
                &endpoint,
                "mailto:op@example.com",
                Duration::from_secs(3600),
                1_000,
            )
            .unwrap();
        let (token, public) = header
            .strip_prefix("vapid t=")
            .and_then(|rest| rest.split_once(", k="))
            .unwrap();
        assert_eq!(decoded(public), key.public_key());
        let (signed, signature) = token.rsplit_once('.').unwrap();
        let (head, claims) = signed.split_once('.').unwrap();
        assert_eq!(decoded(head), br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims: Value = serde_json::from_slice(&decoded(claims)).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["exp"], 4_600);
        assert_eq!(claims["sub"], "mailto:op@example.com");
        let signature = Signature::from_slice(&decoded(signature)).unwrap();
        let verifying = VerifyingKey::from_sec1_bytes(&key.public_key()).unwrap();
        assert!(verifying.verify(signed.as_bytes(), &signature).is_ok());
        let other = VapidKey::from_bytes(&[4; 32]).unwrap();
        let other = VerifyingKey::from_sec1_bytes(&other.public_key()).unwrap();
        assert!(other.verify(signed.as_bytes(), &signature).is_err());

        let long = key
            .authorization(
                &endpoint,
                "mailto:op@example.com",
                Duration::from_secs(172_800),
                0,
            )
            .unwrap();
        let claims = long.split('.').nth(1).unwrap();
        let claims: Value = serde_json::from_slice(&decoded(claims)).unwrap();
        assert_eq!(claims["exp"], 86_400, "a token never outlives a day");
    }

    #[test]
    fn a_key_survives_storage_and_refuses_what_is_not_one() {
        let key = VapidKey::generate().unwrap();
        let restored = VapidKey::from_bytes(&key.to_bytes()).unwrap();
        assert_eq!(restored.public_key(), key.public_key());
        assert_eq!(key.public_key().len(), 65);
        assert!(VapidKey::from_bytes(&[0; 32]).is_err(), "zero is not a key");
        assert!(VapidKey::from_bytes(&[1; 31]).is_err());
        assert!(VapidKey::from_bytes(&[1; 33]).is_err());
    }
}
