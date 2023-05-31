use crate::audit::{Backend, BackendEngine, TokenUsage, TokenUsageLog};
use crate::config::{AuditConfig, StreamTokensPolicy};
use crate::error::ErrorResponse;
use crate::handler::audit::access::RAY_ID_HEADER;
use crate::handler::helpers::stream_read_response_body;
use crate::handler::jwt::AUTHED_HEADER;
use crate::short_circuit_if;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use futures::TryStreamExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::io;
use std::sync::Arc;
use tiktoken_rs::tokenizer::get_tokenizer;
use tiktoken_rs::{get_bpe_from_tokenizer, num_tokens_from_messages, ChatCompletionRequestMessage};
use tokio::io::AsyncReadExt;
use tokio::spawn;
use tokio::sync::mpsc::Receiver;
use tokio_util::io::StreamReader;
use tracing::{event, instrument, Level};

#[instrument(skip_all)]
pub async fn audit_tokens_layer(
    State(state): State<Option<(Arc<AuditConfig>, Backend)>>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorResponse> {
    short_circuit_if!(req, next, state.is_none());

    let (config, backend) = state.unwrap();

    short_circuit_if!(req, next, !config.filters.tokens.enable);
    short_circuit_if!(
        req,
        next,
        !config.filters.tokens.endpoints.contains(req.uri().path())
    );

    let (parts, body) = req.into_parts();
    let user = parts
        .headers
        .get(AUTHED_HEADER)
        .map(|h| h.to_str().unwrap().to_string());
    let ray_id = parts
        .headers
        .get(RAY_ID_HEADER)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let mut req_body = vec![];
    StreamReader::new(body.map_err(|e| io::Error::new(io::ErrorKind::Other, e)))
        .read_to_end(&mut req_body)
        .await
        .map_err(|_| ErrorResponse::new(StatusCode::BAD_REQUEST, "failed to read body"))?;
    let parsed_body: Value = serde_json::from_slice(&req_body)
        .map_err(|_| ErrorResponse::new(StatusCode::BAD_REQUEST, "failed to parse body"))?;
    if parsed_body.get("model").is_none() {
        event!(
            Level::ERROR,
            "tokens statics require 'model' field in request body"
        );
        return Err(ErrorResponse::new(
            StatusCode::BAD_REQUEST,
            "missing 'model' field in request body",
        ));
    }
    let stream = parsed_body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    if stream && config.filters.tokens.stream_tokens == StreamTokensPolicy::Reject {
        return Err(ErrorResponse::new(
            StatusCode::BAD_REQUEST,
            "stream requests are not allowed",
        ));
    }
    let endpoint = parts.uri.path().to_string();

    let request = Request::from_parts(parts, Body::from(req_body));
    let response = next.run(request).await;
    let (response, res_body_rx) = stream_read_response_body(response);

    spawn(audit_tokens_layer_inner(
        endpoint,
        user,
        parsed_body,
        res_body_rx,
        ray_id,
        config,
        backend,
    ));

    Ok(response)
}

async fn audit_tokens_layer_inner(
    endpoint: String,
    user: Option<String>,
    req_body: Value,
    mut res_body_rx: Receiver<Option<Vec<u8>>>,
    ray_id: String,
    config: Arc<AuditConfig>,
    backend: Backend,
) {
    // TODO: stream read response body
    let res_body = res_body_rx
        .recv()
        .await
        .flatten()
        .and_then(|v| String::from_utf8(v).ok());
    if res_body.is_none() {
        event!(Level::WARN, "failed to read response body");
        return;
    }
    let res_body = res_body.unwrap();
    let model = req_body.get("model").unwrap().as_str().unwrap().to_string();

    let stream = req_body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    let (usage, is_estimated) = match (stream, config.filters.tokens.stream_tokens) {
        (true, StreamTokensPolicy::Skip) => return,
        (true, StreamTokensPolicy::Reject) => unreachable!(),
        (true, StreamTokensPolicy::Estimate) => {
            let usage = match endpoint.as_str() {
                "/completions" => count_completions_tokens(model.as_str(), req_body, res_body),
                "/chat/completions" => count_chat_tokens(model.as_str(), req_body, res_body),
                _ => {
                    event!(Level::ERROR, "unsupported endpoint {}", endpoint);
                    return;
                }
            };
            if usage.is_none() {
                event!(
                    Level::WARN,
                    "failed to estimate usage for request, ray id = {}",
                    ray_id
                );
                return;
            }
            (usage.unwrap(), true)
        }
        (false, _) => {
            if let Ok(res) = serde_json::from_str::<ResponseWithUsage>(res_body.as_str()) {
                (res.usage, false)
            } else {
                event!(Level::WARN, "failed to parse usage from response");
                return;
            }
        }
    };

    let log = TokenUsageLog {
        timestamp: chrono::Utc::now(),
        user,
        ray_id,
        model,
        usage,
        is_estimated,
    };
    backend.log_tokens(log).await;
}

fn get_events<T: DeserializeOwned>(res_body: String) -> Option<Vec<StreamEvent<T>>> {
    let events: Result<Vec<StreamEvent<T>>, _> = res_body
        .split("\n\n")
        .filter_map(|event| event.strip_prefix("data: "))
        .filter(|event| *event != "[DONE]")
        .map(|event| {
            event!(Level::DEBUG, "paring event: {}", event);
            serde_json::from_str(event)
        })
        .collect();
    if events.is_err() {
        event!(Level::ERROR, "failed to parse response events");
        return None;
    }
    Some(events.unwrap())
}

fn count_completions_tokens(model: &str, req_body: Value, res_body: String) -> Option<TokenUsage> {
    let tokenizer = get_tokenizer(model)?;
    event!(Level::DEBUG, "got tokenizer {:?} for {}", tokenizer, model);
    let bpe = get_bpe_from_tokenizer(tokenizer).ok()?;

    let prompt_tokens = req_body
        .get("prompt")
        .and_then(|p| p.as_str())
        .map(|s| bpe.encode_with_special_tokens(s).len())
        .unwrap_or(0);

    let events = get_events::<CompletionChoice>(res_body)?;
    let mut choices = vec![];
    for event in events.into_iter() {
        for choice in event.choices.into_iter() {
            if choices.len() < choice.index + 1 {
                choices.resize(choice.index + 1, String::new());
            }
            choices[choice.index].push_str(choice.text.as_str());
        }
    }
    let completion_tokens = choices
        .iter()
        .map(|s| bpe.encode_with_special_tokens(s).len())
        .sum();

    Some(TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    })
}

fn count_chat_tokens(model: &str, req_body: Value, res_body: String) -> Option<TokenUsage> {
    #[derive(Deserialize)]
    struct ChatCompletionRequestMessageDe {
        role: String,
        content: String,
        name: Option<String>,
    }
    let prompt_messages = req_body.get("messages")?;
    let parsed_prompt =
        serde_json::from_value::<Vec<ChatCompletionRequestMessageDe>>(prompt_messages.clone())
            .ok()?;
    let prompt: Vec<ChatCompletionRequestMessage> = parsed_prompt
        .into_iter()
        .map(|p| ChatCompletionRequestMessage {
            role: p.role,
            content: p.content,
            name: p.name,
        })
        .collect();
    let prompt_tokens = num_tokens_from_messages(model, &prompt).ok()?;
    event!(Level::DEBUG, "estimated prompt tokens: {}", prompt_tokens);

    let events = get_events::<ChatChoice>(res_body)?;

    let mut role = String::new();
    let mut choices = vec![];
    for event in events.into_iter() {
        for choice in event.choices.into_iter() {
            if choices.len() < choice.index + 1 {
                choices.resize(choice.index + 1, String::new());
            }
            if let Some(r) = choice.delta.role {
                debug_assert!(role.is_empty());
                role = r;
            }
            if let Some(c) = choice.delta.content {
                choices[choice.index].push_str(c.as_str());
            }
        }
    }
    let completions: Vec<ChatCompletionRequestMessage> = choices
        .into_iter()
        .map(|content| ChatCompletionRequestMessage {
            role: role.clone(),
            content,
            name: None,
        })
        .collect();
    event!(Level::DEBUG, "completions: {:?}", completions);
    let completion_tokens = num_tokens_from_messages(model, &completions).ok()?;
    event!(
        Level::DEBUG,
        "estimated completion tokens: {}",
        completion_tokens
    );

    Some(TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    })
}

#[derive(Deserialize)]
struct ResponseWithUsage {
    usage: TokenUsage,
}

#[derive(Deserialize)]
struct StreamEvent<T> {
    choices: Vec<T>,
}

#[derive(Copy, Clone, Debug, Deserialize)]
enum ObjectType {
    #[serde(rename = "chat.completion.chunk")]
    ChatCompletionChunk,
    #[serde(rename = "text_completion")]
    TextCompletion,
}

#[derive(Deserialize)]
struct ChatChoice {
    pub delta: Delta,
    pub index: usize,
}

#[derive(Default, Deserialize)]
struct Delta {
    pub role: Option<String>,
    pub content: Option<String>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    pub text: String,
    pub index: usize,
}
