//! `humanip-gateway --config gateway.yaml`: run the SOCKS5 and HTTP CONNECT listeners.

use humanip_gateway::{config, server};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.iter().position(|a| a == "--config") {
        Some(i) if i + 1 < args.len() => args[i + 1].clone(),
        _ => {
            eprintln!("usage: humanip-gateway --config <gateway.yaml>");
            std::process::exit(2);
        }
    };
    let yaml = match std::fs::read_to_string(&path) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(2);
        }
    };
    let cfg = match config::parse_yaml(&yaml) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("invalid config {path}: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = server::run(cfg).await {
        eprintln!("humanip-gateway: {e}");
        std::process::exit(1);
    }
}
