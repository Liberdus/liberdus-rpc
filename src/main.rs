mod rpc;
mod methods;
mod config;

use tiny_http::Server;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let _configs = config::Config::load().unwrap_or_else(|err| {
        eprintln!("Failed to load config: {}", err);
        std::process::exit(1);
    });

    let server = Arc::new(Server::http(format!("0.0.0.0:{}", _configs.rpc_http_port)).expect("Failed to start server"));

    println!("JSON-RPC Server is running on http://0.0.0.0:{}", _configs.rpc_http_port);

    // Don't block the main thread by looping in it directly
    tokio::task::spawn_blocking(move || {
        loop {
            let server_clone = server.clone();
            if let Ok(request) = server_clone.recv() {
                tokio::spawn(rpc::handle_request(request));
            }
        }
    });
}


