use chrono::{DateTime, Days, Months, Utc};
use clap::Parser;
use jwt::{RegisteredClaims, SignWithKey};
use openai_hub_core::config::ServerConfig;
use std::fs::read_to_string;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "SUB")]
    subject: Option<String>,
    #[arg(short, long, value_name = "EXP")]
    expiration: Option<String>,
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let config_path = cli.config.unwrap_or_else(|| "config.toml".parse().unwrap());
    if !config_path.exists() {
        eprintln!("Config file not found");
        std::process::exit(1);
    }
    let config =
        ServerConfig::load(&read_to_string(config_path).unwrap()).expect("cannot load config");

    let mut claims = RegisteredClaims::default();
    let utc: DateTime<Utc> = Utc::now();

    if let Some(subject) = cli.subject {
        claims.subject = Some(subject);
    }
    if let Some(expiration) = cli.expiration {
        if !expiration.is_ascii() {
            eprintln!("Invalid expiration time");
            std::process::exit(1);
        }
        let length: u32 = expiration
            .get(..expiration.len() - 1)
            .unwrap()
            .parse()
            .expect("invalid length");
        let unit = expiration.get(expiration.len() - 1..).unwrap();
        let exp = match unit {
            "d" => utc + Days::new(length as u64),
            "m" => utc + Months::new(length),
            "y" => utc + Months::new(length * 12),
            _ => {
                eprintln!("{} is not a valid unit", unit);
                std::process::exit(1);
            }
        };
        claims.expiration = Some(exp.timestamp() as u64);
    }
    claims.issued_at = Some(utc.timestamp() as u64);

    let token_str = claims.sign_with_key(&config.jwt_auth.key).unwrap();
    println!("{}", token_str);
}
