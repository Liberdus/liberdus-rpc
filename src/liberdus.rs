use crate::archivers;
use reqwest;
use serde_json;
use serde;
use rand::prelude::*;
use std::{cmp::Ordering, sync::Arc};
use tokio::sync::RwLock;
use crate::crypto;


#[derive(serde::Deserialize, serde::Serialize)]
#[derive(Clone)]
#[allow(non_snake_case)]
pub struct Consensor{
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub publicKey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub load: Option<NodeLoad>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub rng_bias: Option<f64>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[derive(Clone)]
pub struct NodeLoad{
    pub external: f64,
    pub internal: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[allow(non_snake_case)]
pub struct ConsensorLoadResp{
    pub load: f64,
    pub nodeLoad: NodeLoad,
}

#[allow(non_snake_case)]
#[derive(serde::Deserialize, serde::Serialize)]
struct SignedNodeListResp{
    pub nodeList: Vec<Consensor>,
    pub sign: Signature,
}


#[derive(serde::Deserialize, serde::Serialize)]
pub struct Signature {
    pub owner: String,
    pub sig: String,
}
#[derive(serde::Deserialize, serde::Serialize)]
struct TxInjectResp{
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>
}

pub struct Liberdus {
    active_nodelist: Arc<RwLock<Vec<Consensor>>>,
    archivers: Arc<RwLock<Vec<archivers::Archiver>>>,
    winner_index: Arc<RwLock<usize>>,
    crypto: Arc<crypto::ShardusCrypto>,
    load_distribution_commulative_bias: Arc<RwLock<Vec<f64>>>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct GetAccountResp{
    account: serde_json::Value,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct GetTransactionResp{
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transaction: Option<serde_json::Value>,
}

impl  Liberdus {
    pub fn new (sc: Arc<crypto::ShardusCrypto>, archivers: Arc<RwLock<Vec<archivers::Archiver>>>) -> Self{
        Liberdus{
            winner_index: Arc::new(RwLock::new(0)),
            active_nodelist: Arc::new(RwLock::new(Vec::new())),
            archivers,
            crypto: sc,
            load_distribution_commulative_bias: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn update_active_nodelist(self: Arc<Self>){

        for archiver in self.archivers.read().await.iter(){
            let url = format!("http://{}:{}/full-nodelist?activeOnly=true", archiver.ip, archiver.port);
            let static_self = self.clone();
            let collected_nodelist = match reqwest::get(&url).await {
                Ok(resp) => {
                    let body: Result<SignedNodeListResp, _> = serde_json::from_str(&resp.text().await.unwrap());
                    match body{
                        Ok(body) => {
                            //important that serde doesn't populate default value for
                            // Consensor::load
                            // it'll make the signature go bad.
                            if static_self.verify_signature(&body){
                                Ok(body.nodeList)
                            }
                            else{
                                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid signature"))
                            }
                        },
                        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
                    }
                },
                Err(e) => Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, e.to_string())),
            };
            if collected_nodelist.is_ok(){
                static_self.prepare_list(collected_nodelist.unwrap()).await;
                break
            }

        }

    }

    /// Prepares the active consensor list by fetching and calculating load-based biases.
    /// 
    /// - Fetches the load details for each node in `unordered_incomplete_list` concurrently.
    /// - Sorts nodes by their load (`external + internal`), lowest load node to highest load node.
    /// - Computes `rng_bias` for each node as `1 / (load.external + load.internal)`, capped to avoid division by zero.
    /// - Builds a cumulative bias list for efficient O(log n) random selection.
    /// - Updates `active_nodelist` and `load_distribution_commulative_bias` atomically.
    ///
    /// This function handles errors gracefully, skipping invalid or unreachable nodes, ensuring a healthy active list.
    ///
    /// **Time Complexity**: O(n log n) for sorting and load computation.
    async fn prepare_list(self: Arc<Self>, unordered_incomplete_list: Vec<Consensor>) {
        let long_lived_self = self.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Consensor, std::io::Error>>(64);
    
        tokio::spawn(async move {
            let mut collected_consensors: Vec<Consensor> = Vec::new();
            while let Some(result) = rx.recv().await{
                if result.is_ok(){
                    collected_consensors.push(result.unwrap());
                }
            }
    
            let fallback_bad_load = NodeLoad{
                external: 1.0,
                internal: 1.0,
            };
            collected_consensors.sort_by(|a, b| {
                let a = a.load.as_ref().unwrap_or(&fallback_bad_load);
                let b = b.load.as_ref().unwrap_or(&fallback_bad_load);
                let a_score = a.external + a.internal;
                let b_score = b.external + b.internal;
                a_score.partial_cmp(&b_score).unwrap_or(Ordering::Equal)
            });

            let mut total_bias = 0.0;
            let mut cumulative_bias = Vec::new();
            for i in 0..collected_consensors.len() {
                let load = collected_consensors[i].load.as_ref().unwrap_or(&fallback_bad_load);
                collected_consensors[i].rng_bias = Some(1.0 / (load.external + load.internal).max(0.01)); // Bias formula
                total_bias += collected_consensors[i].rng_bias.unwrap_or(0.0);
                cumulative_bias.push(total_bias);
            }

            {
                let mut guard = long_lived_self.active_nodelist.write().await;
                *guard = collected_consensors;
                drop(guard);
            }

            {
                let mut guard = long_lived_self.load_distribution_commulative_bias.write().await;
                *guard = cumulative_bias;
                drop(guard);
            }

        });
        for node in unordered_incomplete_list.iter() {
            let url = format!("http://{}:{}/load", node.ip, node.port);
            let node = node.clone();
            let transmitter = tx.clone();
            tokio::spawn(async move {
                let resp = match reqwest::get(&url).await {
                    Ok(resp) => {
                        let body: Result<ConsensorLoadResp, _> = serde_json::from_str(&resp.text().await.unwrap());
                        match body{
                            Ok(body) => {
                                let tmp = Consensor{
                                    id: node.id,
                                    ip: node.ip,
                                    port: node.port,
                                    publicKey: node.publicKey,
                                    load: Some(body.nodeLoad),
                                    rng_bias: None,
                                };
                               
                                Ok(tmp)
    
                            },
                            Err(_e) => {
                                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Malfromed Load Object"))
                            },
                        }
                    },
                    Err(_e) => {
                        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Bad Request {}:{}", node.ip, node.port)))
                    }
                };
    
                let _ = transmitter.send(resp).await;
                drop(transmitter);
            });
        }
    
    }
    
    /// Selects a random node from the active list based on weighted bias (`1 / (internal + external)`),
    /// favoring lower-load nodes with biased probabality to be picked. Uses precomputed cumulative bias for O(log n)
    /// binary search. Returns `Some(Consensor)` if nodes exist, else `None`. Efficient for dynamic load-balancing.
    /// use `get_random_consensor` for a non-biased faster random selection.
    pub async fn get_random_consensor_biased(&self) -> Option<Consensor> {

            let nodes = self.active_nodelist.read().await;
            let cumulative_weights = self.load_distribution_commulative_bias.read().await;

            if nodes.is_empty() {
                return None;
            }

            let mut rng = thread_rng();
            let total_bias = *cumulative_weights.last().unwrap();
            let random_value: f64 = rng.gen_range(0.0..total_bias);

            let index = match cumulative_weights.binary_search_by(|&bias| bias.partial_cmp(&random_value).unwrap()) {
                Ok(i) => i,      // Exact match
                Err(i) => i,     // Closest match (next higher value)
            };

            Some(nodes[index].clone())

    }

    pub async fn get_random_consensor(&self) -> Option<Consensor> {
        let nodes = self.active_nodelist.read().await;
        if nodes.is_empty(){
            return None;
        }
        let mut rng = thread_rng();
        let index = rng.gen_range(0..nodes.len());
        Some(nodes[index].clone())
    }


    pub async fn inject_transaction(&self, tx: serde_json::Value) -> Result<serde_json::Value, serde_json::Value>{
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "tx": tx,
        });

        let consensor = match self.get_random_consensor_biased().await {
            Some(consensor) => consensor,
            None => return Err("Failed to select consensor".into()),
        };

         let resp = client.post(&format!("http://{}:{}/inject", consensor.ip, consensor.port))
            .json(&payload)
            .send()
            .await;
        
        match resp{
            Ok(resp) => {
                let body: TxInjectResp = resp.json().await.unwrap();
                if let Some(result) = body.result{
                    Ok(result)
                }
                else{
                    Err(body.error.unwrap())
                }
            },
            Err(e) => Err(e.to_string().into()),
        }
    }

    pub async fn get_account_by_addr(&self, address: String) -> Result<serde_json::Value, serde_json::Value>{

        let consensor = match self.get_random_consensor_biased().await {
            Some(consensor) => consensor,
            None => return Err("Failed to select consensor".into()),
        };

        let resp = reqwest::get(&format!("http://{}:{}/account/{}", consensor.ip, consensor.port, address)).await;

        match resp{
            Ok(resp) => {
                let body: GetAccountResp = resp.json().await.unwrap();
                match body.account{
                    serde_json::Value::Null => Err("Account not found".into()),
                    _ => Ok(body.account),
                }
            },
            Err(e) => Err(e.to_string().into()),
        }
    }

    pub async fn get_transaction_receipt(&self, id: String) -> Result<serde_json::Value, serde_json::Value>{

        let consensor = match self.get_random_consensor_biased().await {
            Some(consensor) => consensor,
            None => return Err("Failed to select consensor".into()),
        };

        let resp = reqwest::get(&format!("http://{}:{}/transaction/{}", consensor.ip, consensor.port, id)).await;

        match resp{
            Ok(resp) => {
                let body: GetTransactionResp = resp.json().await.unwrap();
                match body.transaction{
                    Some(tx) => Ok(tx),
                    None => Err("Transaction not found".into()),
                }

            },
            Err(e) => Err(e.to_string().into()),
        }
    }

    pub async fn get_messages(&self, chat_id: String) -> Result<serde_json::Value, serde_json::Value>{
        let consensor = match self.get_random_consensor_biased().await {
            Some(consensor) => consensor,
            None => return Err("Failed to select consensor".into()),
        };

        let resp = reqwest::get(&format!("http://{}:{}/messages/{}", consensor.ip, consensor.port, chat_id)).await;

        match resp{
            Ok(resp) => {
                let body: serde_json::Value = resp.json().await.unwrap();
                Ok(body)
            },
            Err(e) => Err(e.to_string().into()),
        }
    }

    fn verify_signature(&self, signed_payload: &SignedNodeListResp) -> bool {
        let unsigned_msg = serde_json::json!({
            "nodeList": signed_payload.nodeList,
        });

        let hash = self.crypto.hash(&unsigned_msg.to_string().into_bytes(), crate::crypto::Format::Hex);

        let pk = sodiumoxide::crypto::sign::PublicKey::from_slice(
            &sodiumoxide::hex::decode(&signed_payload.sign.owner).unwrap()
        ).unwrap();

        self.crypto.verify(&hash, &sodiumoxide::hex::decode(&signed_payload.sign.sig).unwrap().to_vec(), &pk)

    }
}

