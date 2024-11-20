
use std::env;
use std::fs;

#[derive(Debug, serde::Deserialize)]
pub struct Config{
    pub rpc_http_port: u16,
    pub archiver_ip: String,
    pub archiver_port: u16,
    pub debug: bool,
}


impl Config {
    pub fn load() -> Result<Self, String> {
        let config_file = format!("src/config.json");

        let config_data = fs::read_to_string(&config_file)
            .map_err(|err| format!("Failed to read config file: {}", err))?;

        serde_json::from_str(&config_data)
            .map_err(|err| format!("Failed to parse config file: {}", err))
    }

    pub fn override_with_env(mut self) -> Self {
        if let Ok(port) = env::var("SERVER_PORT") {
            self.rpc_http_port = port.parse().unwrap_or(self.rpc_http_port);
        }
        if let Ok(ip) = env::var("ARCHIVER_IP") {
            self.archiver_ip = ip;
        }
        if let Ok(port) = env::var("ARCHIVER_PORT") {
            self.archiver_port = port.parse().unwrap_or(self.archiver_port);
        }
        if let Ok(debug) = env::var("DEBUG") {
            self.debug = debug == "true";
        }
        self
    }
}


