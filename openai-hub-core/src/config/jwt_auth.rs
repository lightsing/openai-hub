use hmac::digest::KeyInit;
use hmac::Hmac;
use serde::Deserialize;
use sha2::Sha256;

#[derive(Clone, Debug)]
pub struct JwtAuthConfig {
    pub key: Hmac<Sha256>,
}

#[derive(Clone, Deserialize)]
pub struct JwtAuthConfigDe {
    pub secret: String,
}

impl From<JwtAuthConfigDe> for JwtAuthConfig {
    fn from(de: JwtAuthConfigDe) -> Self {
        let key = Hmac::new_from_slice(de.secret.as_bytes()).unwrap();
        Self { key }
    }
}
