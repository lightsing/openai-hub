use openai_hub::config::ServerConfig;
use openai_hub::Server;
use std::fs::read_to_string;

#[cfg(feature = "acl")]
use openai_hub::ApiAcl;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let config = ServerConfig::load(&read_to_string("config.toml").unwrap())?;

    #[cfg(feature = "acl")]
    config.set_global_api_acl(ApiAcl::load(&read_to_string("acl.toml").unwrap())?);

    Server::from_config(config).serve().await?;
    Ok(())
}
