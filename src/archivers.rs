use reqwest;
use sodiumoxide;
use tokio::sync::RwLock;
use crate::crypto::ShardusCrypto;
use std::sync::Arc;
use rand::prelude::*;

pub struct ArchiverUtil {
    seed_list: Vec<Archiver>,
    final_list: Arc<RwLock<Vec<Archiver>>>,
    crypto: ShardusCrypto,
}

#[derive(serde::Deserialize, serde::Serialize)]
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

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignedArchiverListResponse {
    activeArchivers: Vec<Archiver>,
    sign: Signature,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Signature {
    owner: String,
    sig: String,
}

impl ArchiverUtil {
    pub fn new(key: &str, seed: Vec<Archiver>) -> Self {
        ArchiverUtil { 
            seed_list: seed,
            final_list: Arc::new(RwLock::new(Vec::new())),
            crypto: ShardusCrypto::new(key),
        }
    }

    pub async fn discover(&self) {
        // todo improve performance
        for seed in self.seed_list.as_slice() {
            let resp = reqwest::get(&format!("http://{}:{}/archivers", seed.ip, seed.port)).await.unwrap();
            let body: Result<SignedArchiverListResponse, _> = serde_json::from_str(&resp.text().await.unwrap());
            match body {
                Ok(body) => {

                    if self.verify_signature(&body) {
                        let mut guard = self.final_list.write().await;
                        *guard = body.activeArchivers;
                        drop(guard);
                    }

                }
                Err(_) => continue,
            }
        }

        self.remove_duplicates().await;
    }

    async fn remove_duplicates(&self) {
        let mut final_list = self.final_list.write().await;
        final_list.dedup_by(|a, b| a.publicKey == b.publicKey);
        drop(final_list);
    }

    pub fn get_active_archivers(&self) -> Arc<RwLock<Vec<Archiver>>> {
        self.final_list.clone()
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

   // Uniform distributed thread-safe random selection

}

pub async fn select_random_archiver(archivers: Arc<RwLock<Vec<Archiver>>>) -> Option<Archiver> {
    let guard = archivers.read().await; 
    if guard.is_empty() {
        return None; // Return None if the list is empty
    }
    let index = rand::thread_rng().gen_range(0..guard.len()); // Generate a random index
    Some(guard[index].clone()) // Return the selected archiver
}
