use chrono::{DateTime, Days, Months, Utc};
use clap::Parser;
use hmac::digest::KeyInit;
use hmac::Hmac;
use jwt::SignWithKey;
use openai_hub_core::config::ServerConfig;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs::read_to_string;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "SUB")]
    sub: Option<String>,
    #[arg(short, long, value_name = "EXP")]
    exp: Option<String>,
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

    let key: Hmac<Sha256> = Hmac::new_from_slice(config.jwt_auth.secret.as_bytes()).unwrap();
    let mut claims = BTreeMap::new();
    let utc: DateTime<Utc> = Utc::now();

    if let Some(sub) = cli.sub {
        claims.insert("sub", sub);
    }
    if let Some(exp) = cli.exp {
        if !exp.is_ascii() {
            eprintln!("Invalid expiration time");
            std::process::exit(1);
        }
        let length: u32 = exp
            .get(..exp.len() - 1)
            .unwrap()
            .parse()
            .expect("invalid length");
        let unit = exp.get(exp.len() - 1..).unwrap();
        let exp = match unit {
            "d" => utc + Days::new(length as u64),
            "m" => utc + Months::new(length),
            "y" => utc + Months::new(length * 12),
            _ => {
                eprintln!("{} is not a valid unit", unit);
                std::process::exit(1);
            }
        };
        claims.insert("exp", exp.timestamp().to_string());
    }
    claims.insert("iat", utc.timestamp().to_string());

    let token_str = claims.sign_with_key(&key).unwrap();
    println!("{}", token_str);
}
