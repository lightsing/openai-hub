use openai_hub::acl::ApiAcl;
use openai_hub::config::ServerConfig;
use openai_hub::Server;
use std::fs::read_to_string;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let acl = ApiAcl::load(&read_to_string("acl.toml").unwrap())?;
    let config = ServerConfig::load(&read_to_string("config.toml").unwrap(), acl)?;
    Server::from_config(config).serve().await?;
    Ok(())
}
