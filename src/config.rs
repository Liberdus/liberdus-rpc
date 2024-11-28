use std::fs;

#[derive(Debug, serde::Deserialize)]
#[derive(Clone)]
pub struct Config{
    pub rpc_http_port: u16,
    pub archiver_seed_path: String,
    pub nodelist_refresh_interval_sec: u64,
    pub debug: bool,
    pub max_http_timeout_ms: u128,
    pub collector: CollectorConfig,
}

#[derive(Debug, serde::Deserialize)]
#[derive(Clone)]
pub struct CollectorConfig {
    pub ip: String,
    pub port: u16,
}


impl Config {
    pub fn load() -> Result<Self, String> {
        let config_file = format!("src/config.json");

        let config_data = fs::read_to_string(&config_file)
            .map_err(|err| format!("Failed to read config file: {}", err))?;

        serde_json::from_str(&config_data)
            .map_err(|err| format!("Failed to parse config file: {}", err))
    }

}


