use crate::acl::ApiAcl;
use serde::{Deserialize, Serialize};
use std::net::{AddrParseError, SocketAddr};


#[derive(Clone)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub api_keys: Vec<String>,
    pub organization: Option<String>,
    pub api_base: String,
    pub api_type: ApiType,
    pub api_version: Option<String>,
    pub global_api_acl: ApiAcl,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
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
    pub fn load(s: &str, api_acl: ApiAcl) -> Result<Self, LoadError> {
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
        }
        let ConfigDe {
            bind,
            api_keys,
            organization,
            api_base,
            api_type,
            api_version,
        } = toml::from_str(s)?;
        Ok(Self {
            addr: bind.parse()?,
            api_keys,
            organization,
            api_base: api_base.unwrap_or("https://api.openai.com/v1".to_string()),
            api_type,
            api_version,
            global_api_acl: api_acl,
        })
    }
}


