use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use blake2b_simd::Params;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::RelayError;

type HmacSha256 = Hmac<Sha256>;

pub struct Secrets {
    hmac_key: Vec<u8>,
    uid: Aes256Gcm,
}

impl Secrets {
    pub fn new(hmac_key: &str, uid_key: &str) -> Result<Self, RelayError> {
        let hmac_key = hex::decode(hmac_key)
            .map_err(|error| RelayError::ConfigInvalid(format!("hmac_key: {error}")))?;
        let uid_key =
            hex::decode(uid_key).map_err(|error| RelayError::ConfigInvalid(format!("uid_key: {error}")))?;
        if uid_key.len() != 32 {
            return Err(RelayError::ConfigInvalid("uid_key must be 32 bytes".into()));
        }
        Ok(Self {
            hmac_key,
            uid: Aes256Gcm::new_from_slice(&uid_key)
                .map_err(|error| RelayError::ConfigInvalid(format!("uid_key: {error}")))?,
        })
    }

    fn mac(&self, message: &[u8]) -> [u8; 32] {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.hmac_key)
            .expect("hmac accepts any key length");
        mac.update(message);
        mac.finalize().into_bytes().into()
    }

    pub fn mask_hashes(&self, ip: u32) -> [[u8; 32]; 4] {
        let masks: [(u8, u32); 4] = [
            (32, 0xFFFFFFFF),
            (24, 0xFFFFFF00),
            (20, 0xFFFFF000),
            (16, 0xFFFF0000),
        ];
        let mut hashes = [[0u8; 32]; 4];
        for (index, (mask_byte, mask)) in masks.iter().enumerate() {
            let mut message = vec![*mask_byte];
            message.extend_from_slice(&(ip & mask).to_be_bytes());
            hashes[index] = self.mac(&message);
        }
        hashes
    }

    pub fn uid(&self, hashes: &[[u8; 32]; 4]) -> Result<Vec<u8>, RelayError> {
        let plain = bcs::to_bytes(hashes).map_err(|error| RelayError::Internal(error.to_string()))?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .uid
            .encrypt(&nonce, plain.as_slice())
            .map_err(|_| RelayError::Internal("uid encrypt".into()))?;
        let mut output = nonce.to_vec();
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn unseal_uid(&self, uid: &[u8]) -> Result<[[u8; 32]; 4], RelayError> {
        if uid.len() < 12 {
            return Err(RelayError::BadRequest("uid too short".into()));
        }
        let nonce_bytes: [u8; 12] = uid[..12]
            .try_into()
            .map_err(|_| RelayError::BadRequest("uid nonce".into()))?;
        let nonce = Nonce::from(nonce_bytes);
        let plain = self
            .uid
            .decrypt(&nonce, &uid[12..])
            .map_err(|_| RelayError::BadRequest("uid decrypt".into()))?;
        bcs::from_bytes(&plain).map_err(|_| RelayError::BadRequest("uid decode".into()))
    }

    pub fn ip32(&self, ip: u32, domain: Option<&[u8]>) -> [u8; 32] {
        let mut message = Vec::with_capacity(36);
        if let Some(domain) = domain {
            message.extend_from_slice(domain);
        }
        message.extend_from_slice(&ip.to_be_bytes());
        self.mac(&message)
    }

    pub fn ban_digest(&self, mask: u8, hash: &[u8; 32]) -> [u8; 32] {
        let mut input = b"ban-seal".to_vec();
        input.push(mask);
        input.extend_from_slice(hash);
        let digest = Params::new().hash_length(32).hash(&input);
        let mut output = [0u8; 32];
        output.copy_from_slice(digest.as_bytes());
        output
    }
}
