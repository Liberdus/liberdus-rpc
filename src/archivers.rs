//! # Archiver Utility Module
//! 
//! This module provides functionality for managing and discovering archivers in the network. 
//! It includes methods for loading archivers, verifying their authenticity, and maintaining 
//! an active list of reachable archivers. The module is designed to work asynchronously, 
//! allowing seamless integration in a highly concurrent environment.
use reqwest;
use crate::config;
use sodiumoxide;
use tokio::{io::AsyncWriteExt, sync::RwLock};
use crate::crypto::ShardusCrypto;
use std::sync::Arc;
use std::fs;

pub struct ArchiverUtil {
    config: config::Config,
    seed_list: Arc<RwLock<Vec<Archiver>>>,
    active_archivers: Arc<RwLock<Vec<Archiver>>>,
    crypto: Arc<ShardusCrypto>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[allow(non_snake_case)]
pub struct Archiver {
    pub publicKey: String,
    pub port: u16,
    pub ip: String,
}

impl Clone for Archiver {
    fn clone(&self) -> Self {
        Archiver {
            publicKey: self.publicKey.clone(),
            port: self.port,
            ip: self.ip.clone(),
        }
    }
}

#[allow(non_snake_case)]
#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignedArchiverListResponse {
    activeArchivers: Vec<Archiver>,
    sign: Signature,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Signature {
    pub owner: String,
    pub sig: String,
}

impl ArchiverUtil {
    pub fn new(sc: Arc<ShardusCrypto>, seed: Vec<Archiver>, config: config::Config) -> Self {
        ArchiverUtil { 
            config,
            seed_list: Arc::new(RwLock::new(seed)),
            active_archivers: Arc::new(RwLock::new(Vec::new())),
            crypto: sc,
        }
    }

    /// Discovers active archivers in the network.
    /// 
    /// This method fetches the archiver lists from known seed nodes, validates their signatures,
    /// and updates the active list. It also caches the results to a file for future use.
    /// 
    /// # Process
    /// 1. Load cached archiver data from a local file.
    /// 2. Combine the cached data with the seed list to form a discovery base.
    /// 3. Query each archiver in the list for its view of the active network.
    /// 4. Validate the cryptographic signature of each response.
    /// 5. Deduplicate the archiver list by public key.
    /// 6. Update the active archiver list and persist it back to the cache file.
    ///
    /// This function mutate the archiver list shared across the application. Meaning that it will
    /// inevitably lock the list but it is optimized to minimize the time the lock is held.
    pub async fn discover(self: Arc<Self>) {
         
        let mut cache:Vec<Archiver> = match fs::read_to_string("known_archiver_cache.json") {
            Ok(cache) => { 
                match serde_json::from_str(&cache) {
                    Ok(cache) => cache,
                    Err(_) => Vec::new(),
                }
            },
            Err(_) => Vec::new(),
        };
        

        cache.extend(self.seed_list.read().await.clone());
        cache.dedup_by(|a, b| a.publicKey == b.publicKey);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<Vec<Archiver>, std::io::Error>>();

        let long_lived_self = self.clone();


        tokio::spawn(async move {
            for offline_combined_list in cache.as_slice() {
                let url = format!("http://{}:{}/archivers", offline_combined_list.ip, offline_combined_list.port);
                let transmitter = tx.clone();
                let long_lived_self = self.clone();

                tokio::spawn(async move {
                    let resp = match reqwest::get(url).await {
                        Ok(resp) => { 
                            let body: Result<SignedArchiverListResponse, _> = serde_json::from_str(&resp.text().await.unwrap());
                            match body {
                                Ok(body) => {
                                    if long_lived_self.verify_signature(&body) {
                                        Ok(body.activeArchivers)
                                    } else {
                                        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid signature"))
                                    }
                                },
                                Err(_) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Malformed Json")),
                            }
                        },
                        Err(_) => {
                            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid response"))
                        }
                    };
                    let _ = transmitter.send(resp);
                    drop(transmitter);
                });
            }
        });

        let mut tmp: Vec<Archiver> = Vec::new(); 
        while let Some(result) = rx.recv().await {
            match result {
                Ok(archivers) => {
                    tmp.extend(archivers);
                },
                Err(_) => {},
            }
        }


        tmp.dedup_by(|a, b| a.publicKey == b.publicKey);


        if long_lived_self.config.standalone_network.enabled {
            for archiver in tmp.iter_mut() {
                archiver.ip = long_lived_self.config.standalone_network.replacement_ip.clone();
            }
        }

        let dump = tmp.clone();

        {
            let mut guard = long_lived_self.active_archivers.write().await;
            *guard = tmp;
            drop(guard);
        }

        tokio::spawn(async move {
            let mut file = tokio::fs::File::create("known_archiver_cache.json").await.unwrap();
            let data = serde_json::to_string(&dump).unwrap();
            file.write_all(data.as_bytes()).await.unwrap();
        });

    }

    pub fn get_active_archivers(&self) -> Arc<RwLock<Vec<Archiver>>> {
        self.active_archivers.clone()
    }


    fn verify_signature(&self, signed_payload: &SignedArchiverListResponse) -> bool {
        let unsigned_msg = serde_json::json!({
            "activeArchivers": signed_payload.activeArchivers,
        });

        let hash = self.crypto.hash(&unsigned_msg.to_string().into_bytes(), crate::crypto::Format::Hex);

        let pk = sodiumoxide::crypto::sign::PublicKey::from_slice(
            &sodiumoxide::hex::decode(&signed_payload.sign.owner).unwrap()
        ).unwrap();

        self.crypto.verify(&hash, &sodiumoxide::hex::decode(&signed_payload.sign.sig).unwrap().to_vec(), &pk)

    }
}
