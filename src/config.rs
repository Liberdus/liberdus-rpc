//! Configuration for the rpc server.
use std::fs;

#[derive(Debug, serde::Deserialize)]
#[derive(Clone)]
pub struct Config{
    /// The port on which the RPC server will listen
    pub rpc_http_port: u16,

    /// This is a system directory path to the archiver seed file
    /// Archiver seed file should contain a list of reputable and reliable node
    pub archiver_seed_path: String,

    /// The interval in seconds at which the node list will be refreshed
    pub nodelist_refresh_interval_sec: u64,

    /// This is currently not used anywhere
    pub debug: bool,

    /// The maximum time in milliseconds that the rpc will cancel a request when communication with
    /// collector or consensus node
    pub max_http_timeout_ms: u128,

    /// Collector configuration
    pub collector: CollectorConfig,

    /// Standalone network configuration
    pub standalone_network: StandaloneNetworkConfig,
}



#[derive(Debug, serde::Deserialize)]
#[derive(Clone)]
pub struct CollectorConfig {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, serde::Deserialize)]
#[derive(Clone)]
/// Standalone network mean that consensus node and archivers reside within same server.
/// This mean archiver will returns node list with loopback ip address 0.0.0.0, 127.0.0.1, localhost.
/// When rpc is on a separate machine, loopback ips of nodes will not work.
/// Usually standalone network config are for testnets. Should not be used in productions
pub struct StandaloneNetworkConfig {
    pub replacement_ip: String,
    pub enabled: bool,
}

/// Load the configuration from the config json file
/// path is src/config.json
impl Config {
    pub fn load() -> Result<Self, String> {
        let config_file = format!("src/config.json");

        let config_data = fs::read_to_string(&config_file)
            .map_err(|err| format!("Failed to read config file: {}", err))?;

        serde_json::from_str(&config_data)
            .map_err(|err| format!("Failed to parse config file: {}", err))
    }

}


