use serde::{Deserialize, Serialize};
use std::net::{AddrParseError, SocketAddr};

#[cfg(feature = "acl")]
use crate::acl::ApiAcl;

#[cfg(feature = "jwt-auth")]
mod jwt_auth;
#[cfg(feature = "jwt-auth")]
pub use jwt_auth::JwtAuthConfig;
#[cfg(feature = "jwt-auth")]
use jwt_auth::JwtAuthConfigDe;

#[cfg(feature = "access-log")]
mod access_log;
#[cfg(feature = "access-log")]
pub use access_log::*;

#[derive(Clone)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub api_keys: Vec<String>,
    pub openai: OpenAIConfig,
    #[cfg(feature = "acl")]
    pub global_api_acl: Option<ApiAcl>,
    #[cfg(feature = "jwt-auth")]
    pub jwt_auth: Option<JwtAuthConfig>,
}

#[derive(Clone)]
pub struct OpenAIConfig {
    pub organization: Option<String>,
    pub api_base: String,
    pub api_type: ApiType,
    pub api_version: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub enum ApiType {
    #[serde(rename = "open_ai")]
    #[default]
    OpenAI,
    #[serde(rename = "azure")]
    Azure,
    #[serde(rename = "azure_ad")]
    AzureAD,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    AddrParse(#[from] AddrParseError),
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
}

impl ServerConfig {
    pub fn load(s: &str) -> Result<Self, LoadError> {
        #[derive(Deserialize)]
        struct ConfigDe {
            bind: String,
            api_keys: Vec<String>,
            #[serde(default)]
            organization: Option<String>,
            #[serde(default)]
            api_base: Option<String>,
            #[serde(default)]
            api_type: ApiType,
            #[serde(default)]
            api_version: Option<String>,
            #[cfg(feature = "jwt-auth")]
            #[serde(rename = "jwt-auth")]
            #[serde(default)]
            jwt_auth: Option<JwtAuthConfigDe>,
            #[cfg(feature = "access-log")]
            #[serde(rename = "access-log")]
            #[serde(default)]
            access_log: Option<AccessLogConfig>,
        }
        let config_de: ConfigDe = toml::from_str(s)?;
        Ok(Self {
            addr: config_de.bind.parse()?,
            api_keys: config_de.api_keys,
            openai: OpenAIConfig {
                organization: config_de.organization,
                api_base: config_de
                    .api_base
                    .unwrap_or("https://api.openai.com/v1".to_string()),
                api_type: config_de.api_type,
                api_version: config_de.api_version,
            },
            #[cfg(feature = "acl")]
            global_api_acl: None,
            #[cfg(feature = "jwt-auth")]
            jwt_auth: config_de.jwt_auth.map(Into::into),
        })
    }

    #[cfg(feature = "acl")]
    pub fn set_global_api_acl(&mut self, acl: ApiAcl) -> &mut Self {
        self.global_api_acl = Some(acl);
        self
    }
}
