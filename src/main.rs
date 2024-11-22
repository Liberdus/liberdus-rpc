mod rpc;
mod methods;
mod config;
mod archivers;
mod liberdus;
mod crypto;

use std::sync::Arc;
use std::fs;
use poem::{
    listener::TcpListener,
    EndpointExt, Route, Server,
};

#[derive(Clone)]
pub struct AppState {
    archiver_utils: Arc<archivers::ArchiverUtil>,
    liberdus: Arc<liberdus::Liberdus>,
}

#[tokio::main]
async fn main()  -> Result<(), std::io::Error>{
    // console_subscriber::init();
    let _configs = config::Config::load().unwrap_or_else(|err| {
        eprintln!("Failed to load config: {}", err);
        std::process::exit(1);
    });

    let archiver_seed_path = format!("src/archiver_seed.json");

    let archiver_seed_data = fs::read_to_string(&archiver_seed_path)
        .map_err(|err| format!("Failed to read archiver seed file: {}", err)).unwrap();

    let archiver_seed: Vec<archivers::Archiver> = serde_json::from_str(&archiver_seed_data).unwrap();

    let crypto = Arc::new(crypto::ShardusCrypto::new("69fa4195670576c0160d660c3be36556ff8d504725be8a59b5a96509e0c994bc"));

    let arch_utils = Arc::new(archivers::ArchiverUtil::new(crypto.clone(), archiver_seed));
    let lbd = Arc::new(liberdus::Liberdus::new(crypto.clone(), arch_utils.get_active_archivers()));


    
    let _liberdus = Arc::clone(&lbd);
    let _archivers = Arc::clone(&arch_utils);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    
        loop {
            ticker.tick().await;
            _archivers.discover().await;
            _liberdus.populate_active_nodelist().await;
    
        }
    });

    let state = AppState {
        archiver_utils: arch_utils,
        liberdus: lbd,
    };


    let app = Route::new()
        .at("/", poem::post(rpc::rpc_handler))
        .data(state);
    
    println!(
        "JSON-RPC Server running on http://127.0.0.1:{}",
        _configs.rpc_http_port
    );
    Server::new(TcpListener::bind((
        "127.0.0.1",
        _configs.rpc_http_port,
    )))
    .run(app)
    .await
    
}


