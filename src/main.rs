//! This is the main module of the JSON-RPC server.
//! 
//! This module sets up and initializes the server, managing shared state, cryptographic utilities,
//! archiver discovery, and routing for both HTTP and WebSocket-based RPC calls.
//! 
//! The server leverages `tokio` for asynchronous operations and `poem` for http.
//! RPC is custom implemented on top of poem.
//! 
//! # Features
//! - Loads configuration and seeds archivers from a predefined file.
//! - Supports HTTP and WebSocket endpoints for handling JSON-RPC requests.
//! - Automatically discovers active archivers and updates nodelists periodically.
//! - Employs a shared state for thread-safe interaction with the `liberdus` backend.
//! - Includes CORS middleware for cross-origin requests.
//! 
//! # Modules
//! - [`rpc`]: Implementation of the JSON-RPC protocol for HTTP and WebSocket. 
//! - [`methods`]: Implements the RPC methods invoked by JSON-RPC requests.
//! - [`config`]: Manages configuration loading and validation.
//! - [`archivers`]: Includes utilities for managing and discovering archivers.
//! - [`liberdus`]: This module includes load aware node selectioin mechnism and chat room
//! subscription manager.
//! - [`crypto`]: Provides shardus cryptographic utilities.
//! - [`collector`]: Utilities for quering chain data from distribution protocol.
//! 
//! # Entry Point
//! The `main` function initializes the server, manages shared state, and launches the application.

mod rpc;
mod methods;
mod config;
mod archivers;
mod liberdus;
mod crypto;
mod collector;

use std::sync::Arc;
use std::fs;
use poem::{
    listener::TcpListener,
    EndpointExt, Route, Server,
};

/// Shared state accessible across multiple threads.
/// 
/// This struct holds the primary `Liberdus` instance, which manages backend
/// operations such as node discovery and chat room subscriptions.
#[derive(Clone)]
pub struct CrossThreadSharedState {
    /// Instance of the `Liberdus` backend library.
    liberdus: Arc<liberdus::Liberdus>,
}

/// The main entry point for the application.
/// 
/// # Overview
/// This function performs the following steps:
/// - Loads configuration from a predefined file.
/// - Initializes cryptographic utilities and archiver management.
/// - Spawns a background task for periodic node discovery and updates.
/// - Sets up HTTP and WebSocket routes for JSON-RPC calls.
/// - Starts the server on a specified port.
/// 
/// # Returns
/// - `Ok(())` if the server starts successfully.
/// - `Err(std::io::Error)` if an I/O error occurs during initialization.
#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let _configs = config::Config::load().unwrap_or_else(|err| {
        eprintln!("Failed to load config: {}", err);
        std::process::exit(1);
    });

    let archiver_seed_data = fs::read_to_string(&_configs.archiver_seed_path)
        .map_err(|err| format!("Failed to read archiver seed file: {}", err))
        .unwrap();

    let archiver_seed: Vec<archivers::Archiver> = serde_json::from_str(&archiver_seed_data).unwrap();

    let crypto = Arc::new(crypto::ShardusCrypto::new(
        "69fa4195670576c0160d660c3be36556ff8d504725be8a59b5a96509e0c994bc",
    ));

    let arch_utils = Arc::new(archivers::ArchiverUtil::new(
        crypto.clone(),
        archiver_seed,
        _configs.clone(),
    ));

    let lbd = Arc::new(liberdus::Liberdus::new(
        crypto.clone(),
        arch_utils.get_active_archivers(),
        _configs.clone(),
    ));

    let _archivers = Arc::clone(&arch_utils);
    let _liberdus = Arc::clone(&lbd);

    tokio::spawn(async move {
        Arc::clone(&_archivers).discover().await;
        _liberdus.update_active_nodelist().await;

        let mut ticker = tokio::time::interval(
            tokio::time::Duration::from_secs(_configs.nodelist_refresh_interval_sec),
        );
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            Arc::clone(&_archivers).discover().await;
            _liberdus.update_active_nodelist().await;
            Arc::clone(&_liberdus).discover_new_chats().await;
        }
    });

    let state = CrossThreadSharedState {
        liberdus: lbd,
    };

    let cors = poem::middleware::Cors::new();

    println!("Waiting for active nodelist to be populated.....");
    loop {
        if Arc::clone(&state.liberdus).active_nodelist.read().await.len() > 0 {
            break;
        }
    }

    let app = Route::new()
        .at("/", poem::post(rpc::http_rpc_handler))
        .at("/ws", poem::get(rpc::ws_rpc_handler))
        .with(cors)
        .data(state);

    let pid = std::process::id();
    println!(
        "JSON-RPC Server running on http://127.0.0.1:{}",
        _configs.rpc_http_port
    );
    println!("Process ID: {}", pid);

    // Start the server on the specified port.
    Server::new(TcpListener::bind((
        "0.0.0.0",
        _configs.rpc_http_port,
    )))
    .run(app)
    .await
}
