//! AES-256-GCM [`Encryptor`] (feature `encryption`).

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use async_trait::async_trait;

use crate::error::TenancyError;
use crate::ports::Encryptor;

const NONCE_LEN: usize = 12;

/// AES-256-GCM encryptor; every seal uses a fresh random nonce stored in front
/// of the ciphertext.
pub struct AesGcmEncryptor {
    cipher: Aes256Gcm,
}

impl AesGcmEncryptor {
    /// Creates the encryptor from a 32-byte key.
    pub fn from_key(key: [u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(&key.into()),
        }
    }
}

#[async_trait]
impl Encryptor for AesGcmEncryptor {
    async fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, TenancyError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|error| TenancyError::Crypto(error.to_string()))?;
        let mut sealed = nonce.to_vec();
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    async fn open(&self, ciphertext: &[u8]) -> Result<Vec<u8>, TenancyError> {
        if ciphertext.len() < NONCE_LEN {
            return Err(TenancyError::Crypto("ciphertext too short".to_string()));
        }
        let (nonce, payload) = ciphertext.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce
            .try_into()
            .map_err(|_| TenancyError::Crypto("invalid nonce".to_string()))?;
        self.cipher
            .decrypt(&Nonce::from(nonce), payload)
            .map_err(|error| TenancyError::Crypto(error.to_string()))
    }
}
