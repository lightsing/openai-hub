use crate::helpers::{endpoints_to_regex, wildcards_to_regex};

use axum::http::{Method, StatusCode};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use tracing::{event, instrument, Level};

static DEPLOYMENT_ID_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^/engines/([^/]+)/.+$"#).unwrap());

#[derive(Clone)]
struct MethodSerde(Method);

impl Eq for MethodSerde {}

impl PartialEq for MethodSerde {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Hash for MethodSerde {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<'de> Deserialize<'de> for MethodSerde {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(http_serde::method::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ApiAcl {
    pub global: Global,
    pub endpoint: HashMap<Method, Regex>,
    pub model_body: HashMap<Method, HashMap<String, ModelOption>>,
    pub model_path: HashMap<Method, Vec<(Regex, ModelOption)>>,
}

#[derive(Debug, Clone)]
pub struct Global {
    pub whitelist: bool,
    pub methods: HashMap<Method, bool>,
    pub allow_deployments: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct ModelOption {
    pub allows: Regex,
    pub disallows: Regex,
    pub allow_omitted: bool,
}

impl Default for ModelOption {
    fn default() -> Self {
        Self {
            allows: Regex::new("^.*$").unwrap(),
            disallows: Regex::new("^$").unwrap(),
            allow_omitted: false,
        }
    }
}

pub trait ModelValidator: Send {
    fn validate_path(&self, _path: &str) -> Result<(), AclError> {
        Ok(())
    }

    fn validate_body(&self, _body: &Value) -> Result<(), AclError> {
        Ok(())
    }
}

impl Default for Global {
    fn default() -> Self {
        Self {
            whitelist: true,
            methods: HashMap::from_iter([(Method::POST, true)]),
            allow_deployments: HashSet::new(),
        }
    }
}

pub enum AclError {
    MethodNotAllowed(Method),
    DeploymentNotAllowed(String),
    EndpointNotAllowed(Method, String),
    ModelNotAllowed(String),
    MissingModel,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    InvalidToml(#[from] toml::de::Error),
    #[error(transparent)]
    InvalidRegex(#[from] regex::Error),
}

impl ApiAcl {
    #[instrument(skip_all)]
    pub fn load(s: &str) -> Result<Self, LoadError> {
        #[derive(Deserialize)]
        struct GlobalDe {
            #[serde(default = "default_true")]
            whitelist: bool,
            #[serde(default)]
            methods: HashMap<MethodSerde, bool>,
            #[serde(default)]
            allow_deployments: HashSet<String>,
        }

        #[derive(Deserialize)]
        struct ModelOptionDe {
            #[serde(default)]
            path: bool,
            #[serde(default)]
            allows: Vec<String>,
            #[serde(default)]
            disallows: Vec<String>,
            #[serde(default)]
            allow_omitted: bool,
        }

        #[derive(Deserialize)]
        struct ApiAclDe {
            pub global: GlobalDe,
            #[serde(default)]
            pub endpoint: HashMap<MethodSerde, BTreeMap<String, bool>>,
            #[serde(default)]
            pub model: HashMap<MethodSerde, HashMap<String, ModelOptionDe>>,
        }

        let ApiAclDe {
            global: global_de,
            endpoint,
            model: model_de,
        } = toml::from_str(s)?;

        let global = Global {
            whitelist: global_de.whitelist,
            methods: global_de
                .methods
                .into_iter()
                .map(|(k, v)| (k.0, v))
                .collect(),
            allow_deployments: global_de.allow_deployments,
        };

        let mut endpoint_regex: HashMap<Method, Regex> = HashMap::new();
        for (method, endpoints) in endpoint.into_iter() {
            let endpoints = endpoints
                .into_iter()
                .filter(|(_, allow)| if global.whitelist { *allow } else { !*allow })
                .map(|(endpoint, _)| endpoint);
            endpoint_regex.insert(method.0, endpoints_to_regex(endpoints)?);
        }

        let mut model_body = HashMap::new();
        let mut model_path = HashMap::new();

        for (method, models) in model_de.into_iter() {
            model_body.insert(method.0.clone(), HashMap::new());
            model_path.insert(method.0.clone(), Vec::new());
            for (path, model_de) in models.into_iter() {
                let option = ModelOption {
                    allows: wildcards_to_regex(model_de.allows.into_iter())?,
                    disallows: wildcards_to_regex(model_de.disallows.into_iter())?,
                    allow_omitted: model_de.allow_omitted,
                };
                if model_de.path {
                    event!(Level::DEBUG, "should be a regex rule: {}", path);
                    let path = path.replace("{model}", "(?P<model>[^/]+)");
                    event!(Level::DEBUG, "transformed regex rule: {}", path);
                    model_path
                        .get_mut(&method.0)
                        .unwrap()
                        .push((Regex::new(&path).unwrap(), option));
                } else {
                    event!(Level::DEBUG, "seems to be a normal rule: {}", path);
                    model_body.get_mut(&method.0).unwrap().insert(path, option);
                }
            }
        }

        Ok(Self {
            global,
            endpoint: endpoint_regex,
            model_body,
            model_path,
        })
    }

    #[instrument(skip_all)]
    pub fn validate(
        &self,
        method: &Method,
        path: &str,
    ) -> Result<Option<Box<dyn ModelValidator>>, AclError> {
        // global method check
        event!(
            Level::DEBUG,
            "method: {}, config: {:?}",
            method,
            self.global.methods.get(method)
        );
        if !self.global.methods.get(method).unwrap_or(&false) {
            event!(Level::DEBUG, "method not allowed: {:?}", method);
            return Err(AclError::MethodNotAllowed(method.clone()));
        }
        event!(Level::DEBUG, "path: {}", path);

        // deployment check
        let endpoint = if let Some(deployment_id) = DEPLOYMENT_ID_REGEX.captures(path) {
            let id = deployment_id.get(1).unwrap();
            event!(
                Level::DEBUG,
                "seems contains deployment id: {}",
                id.as_str()
            );
            if !self.global.allow_deployments.contains(id.as_str()) {
                event!(Level::DEBUG, "deployment {} not allowed", id.as_str());
                return Err(AclError::DeploymentNotAllowed(id.as_str().to_string()));
            }
            &path[id.end()..]
        } else {
            path
        };
        event!(Level::DEBUG, "endpoint: {}", endpoint);

        // per endpoint check
        let matched = self
            .endpoint
            .get(method)
            .map(|re| re.is_match(endpoint))
            .unwrap_or(false);
        event!(Level::DEBUG, "rule matched: {}", matched);
        if (self.global.whitelist && !matched) || (!self.global.whitelist && matched) {
            event!(
                Level::DEBUG,
                "endpoint not allowed: {} {}",
                method,
                endpoint
            );
            return Err(AclError::EndpointNotAllowed(
                method.clone(),
                endpoint.to_string(),
            ));
        }

        Ok(self
            .model_body
            .get(method)
            .and_then(|per_method| {
                per_method
                    .get(endpoint)
                    .map(|o| Box::new(o.clone()) as Box<dyn ModelValidator>)
            })
            .or_else(|| {
                self.model_path.get(method).and_then(|regexes| {
                    event!(Level::DEBUG, "not found in plain rules, try regexes");
                    regexes
                        .iter()
                        .find(|(re, _)| re.is_match(endpoint))
                        .map(|o| Box::new(o.clone()) as Box<dyn ModelValidator>)
                })
            }))
    }
}

impl ModelOption {
    #[instrument(skip(self))]
    fn validate(&self, model: Option<&str>) -> Result<(), AclError> {
        match model {
            None => {
                if self.allow_omitted {
                    event!(Level::DEBUG, "model is omitted and allowed");
                    Ok(())
                } else {
                    event!(Level::DEBUG, "model is missing");
                    Err(AclError::MissingModel)
                }
            }
            Some(model) => {
                if self.disallows.is_match(model) || !self.allows.is_match(model) {
                    event!(Level::DEBUG, "model is not allowed");
                    Err(AclError::ModelNotAllowed(model.to_string()))
                } else {
                    event!(Level::DEBUG, "model is allowed");
                    Ok(())
                }
            }
        }
    }
}

impl ModelValidator for (Regex, ModelOption) {
    #[instrument(skip(self))]
    fn validate_path(&self, path: &str) -> Result<(), AclError> {
        debug_assert!(self.0.is_match(path));
        let model = self
            .0
            .captures(path)
            .unwrap()
            .name("model")
            .unwrap()
            .as_str();
        self.1.validate(Some(model))
    }
}

impl ModelValidator for ModelOption {
    #[instrument(skip(self))]
    fn validate_body(&self, body: &Value) -> Result<(), AclError> {
        self.validate(body.get("model").and_then(|m| m.as_str()))
    }
}

impl AclError {
    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            AclError::MethodNotAllowed(_) => StatusCode::METHOD_NOT_ALLOWED,
            _ => StatusCode::FORBIDDEN,
        }
    }
}

impl ToString for AclError {
    fn to_string(&self) -> String {
        match self {
            AclError::MethodNotAllowed(method) => format!("Method {} not allowed", method.as_str()),
            AclError::DeploymentNotAllowed(id) => format!("Deployment {} not allowed", id),
            AclError::EndpointNotAllowed(method, endpoint) => {
                format!("Endpoint {} {} not allowed", method.as_str(), endpoint)
            }
            AclError::ModelNotAllowed(model) => {
                format!("Model {} not allowed", model)
            }
            AclError::MissingModel => "Missing model".to_string(),
        }
    }
}

const fn default_true() -> bool {
    true
}
