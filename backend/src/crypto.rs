use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct KeyStore {
    master: Zeroizing<[u8; 32]>,
}
impl KeyStore {
    fn from_master(master: [u8; 32]) -> Self {
        Self {
            master: Zeroizing::new(master),
        }
    }
    pub fn testing() -> Self {
        Self::from_master([7; 32])
    }
    pub fn at(path: &Path) -> Result<Self, String> {
        if std::fs::exists(path).map_err(|e| e.to_string())? {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let decoded = BASE64.decode(bytes).map_err(|e| e.to_string())?;
            return Ok(Self::from_master(
                decoded
                    .try_into()
                    .map_err(|_| "invalid master key".to_string())?,
            ));
        }
        let mut master = [0; 32];
        OsRng.fill_bytes(&mut master);
        std::fs::write(path, BASE64.encode(master)).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(Self::from_master(master))
    }
    pub fn production() -> Result<Self, String> {
        let entry =
            keyring::Entry::new("dev.sepia.gateway", "master-key").map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(encoded) => {
                let bytes = BASE64.decode(encoded).map_err(|e| e.to_string())?;
                Ok(Self::from_master(
                    bytes
                        .try_into()
                        .map_err(|_| "invalid keyring master key".to_string())?,
                ))
            }
            Err(keyring::Error::NoEntry) => {
                let mut master = [0; 32];
                OsRng.fill_bytes(&mut master);
                entry
                    .set_password(&BASE64.encode(master))
                    .map_err(|e| e.to_string())?;
                Ok(Self::from_master(master))
            }
            Err(error) => Err(format!("OS keyring unavailable: {error}")),
        }
    }
    pub fn production_with_override(encoded: Option<&str>) -> Result<Self, String> {
        let Some(encoded) = encoded else {
            return Self::production();
        };
        let bytes = BASE64
            .decode(encoded)
            .map_err(|_| "invalid master key".to_string())?;
        Ok(Self::from_master(
            bytes
                .try_into()
                .map_err(|_| "invalid master key".to_string())?,
        ))
    }
    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&*self.master).expect("32 byte key")
    }
    pub fn encrypt(&self, plaintext: &str) -> Result<(String, String), String> {
        let mut nonce = [0; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher()
            .encrypt(&nonce.into(), plaintext.as_bytes())
            .map_err(|_| "encryption failed".to_string())?;
        Ok((BASE64.encode(nonce), BASE64.encode(ciphertext)))
    }
    pub fn decrypt(&self, nonce: &str, ciphertext: &str) -> Result<Zeroizing<String>, String> {
        let nonce: [u8; 12] = BASE64
            .decode(nonce)
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|_| "invalid nonce".to_string())?;
        let ciphertext = BASE64.decode(ciphertext).map_err(|e| e.to_string())?;
        let plaintext = self
            .cipher()
            .decrypt(&nonce.into(), ciphertext.as_ref())
            .map_err(|_| "decryption failed".to_string())?;
        Ok(Zeroizing::new(
            String::from_utf8(plaintext).map_err(|e| e.to_string())?,
        ))
    }
}

pub fn random_token(prefix: &str) -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", hex::encode(bytes))
}
pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
pub fn password_hash(password: &str) -> Result<String, String> {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|e| e.to_string())?
        .to_string())
}
pub fn verify_password(hash: &str, password: &str) -> bool {
    use argon2::{Argon2, PasswordVerifier, password_hash::PasswordHash};
    PasswordHash::new(hash).ok().is_some_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}
pub fn default_data_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sepia")
}
