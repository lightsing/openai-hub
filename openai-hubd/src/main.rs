use clap::Parser;
use openai_hub_core::config::ServerConfig;
use openai_hub_core::Server;
use std::fs::read_to_string;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[cfg(feature = "acl")]
use openai_hub_core::ApiAcl;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    #[cfg(feature = "acl")]
    #[arg(short, long, value_name = "FILE")]
    acl: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive("openai_hub_core=debug".parse().unwrap())
                .from_env_lossy(),
        )
        .init();

    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(|| PathBuf::from("config.toml"));

    #[allow(unused_mut)]
    let mut config = ServerConfig::load(&read_to_string(config_path).unwrap())?;

    #[cfg(feature = "acl")]
    {
        let acl_path = cli.acl.unwrap_or_else(|| PathBuf::from("acl.toml"));
        if let Ok(acl) = read_to_string(acl_path) {
            config.set_global_api_acl(ApiAcl::load(&acl)?);
        }
    }

    Server::from_config(config).serve().await?;
    Ok(())
}
