use crate::archivers;
use reqwest;
use serde_json;
use serde;
use rand::prelude::*;
use std::{cmp::Ordering, collections::HashMap, sync::{atomic::{AtomicBool, AtomicU8, AtomicUsize}, Arc}};
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
    pub rng_bias: Option<u128>,
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
    active_sorted_nodelist: Arc<RwLock<Vec<Consensor>>>,
    trip_ms: Arc<RwLock<HashMap<String, u128>>>,
    archivers: Arc<RwLock<Vec<archivers::Archiver>>>,
    round_robin_index: Arc<AtomicUsize>,
    list_prepared: Arc<AtomicBool>,
    crypto: Arc<crypto::ShardusCrypto>,
    load_distribution_commulative_bias: Arc<RwLock<Vec<u128>>>,
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
            round_robin_index: Arc::new(AtomicUsize::new(0)),
            trip_ms: Arc::new(RwLock::new(HashMap::new())),
            active_nodelist: Arc::new(RwLock::new(Vec::new())),
            active_sorted_nodelist: Arc::new(RwLock::new(Vec::new())),
            list_prepared: Arc::new(AtomicBool::new(false)),
            archivers,
            crypto: sc,
            load_distribution_commulative_bias: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn update_active_nodelist(&self){

        let archivers = self.archivers.read().await.clone();

        for archiver in archivers.iter(){
            let url = format!("http://{}:{}/full-nodelist?activeOnly=true", archiver.ip, archiver.port);
            let collected_nodelist = match reqwest::get(&url).await {
                Ok(resp) => {
                    let body: Result<SignedNodeListResp, _> = serde_json::from_str(&resp.text().await.unwrap());
                    match body{
                        Ok(body) => {
                            //important that serde doesn't populate default value for
                            // Consensor::trip_ms
                            // it'll taint the signature payload
                            if self.verify_signature(&body){
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

            match collected_nodelist{
                Ok(nodelist) => {
                    {
                       let mut guard = self.active_nodelist.write().await; 
                       *guard = nodelist;
                    }

                   // inititally node list does not contain load data.
                   self.list_prepared.store(false, std::sync::atomic::Ordering::Relaxed);
                    {
                        let mut guard = self.load_distribution_commulative_bias.write().await;
                        *guard = Vec::new();
                    }
                    {
                        let mut guard = self.trip_ms.write().await;
                        *guard = HashMap::new();
                    }
                   break
                },
                Err(_e) => {
                    continue;
                }
            }


        }

    }
    async fn prepare_list(&self) {
        if self.list_prepared.load(std::sync::atomic::Ordering::Relaxed) == true {
            return;
        }

        let nodes = {
            let guard = self.active_nodelist.read().await;
            guard.clone()
        };

        let trip_ms = {
            let guard = self.trip_ms.read().await;
            guard.clone()
        };

        let fallback_bad_load = 3000; // 3 seconds
        let mut sorted_nodes = nodes.clone();

        sorted_nodes.sort_by(|a, b| {
            let a_time = trip_ms.get(&a.id).unwrap_or(&fallback_bad_load);
            let b_time = trip_ms.get(&b.id).unwrap_or(&fallback_bad_load);
            a_time.cmp(b_time)
        });

        let mut total_bias = 0;
        let mut cumulative_bias = Vec::new();
        for node in &mut sorted_nodes {
            let load = *trip_ms.get(&node.id).unwrap_or(&fallback_bad_load);
            node.rng_bias = Some(1 / load.max(1)); // Bias formula
            total_bias += node.rng_bias.unwrap_or(0);
            cumulative_bias.push(total_bias);
        }

        {
            let mut guard = self.active_nodelist.write().await;
            *guard = sorted_nodes;
        }

        {
            let mut guard = self.load_distribution_commulative_bias.write().await;
            *guard = cumulative_bias;
        }

        self.list_prepared.store(true, std::sync::atomic::Ordering::Relaxed);
    }


    /// Selects a random node from the active list based on weighted bias
    /// bias is derived from last http call's round trip time to the node.
    /// this function required the list to be sorted and bias values are calculated prior
    /// return None otherwise.
    async fn get_random_consensor_biased(&self) -> Option<(usize, Consensor)> {
            if self.list_prepared.load(std::sync::atomic::Ordering::Relaxed) == false {
                return None;
            }

            let nodes = self.active_nodelist.read().await.clone();
            let cumulative_weights = self.load_distribution_commulative_bias.read().await.clone();

            if nodes.is_empty() || cumulative_weights.is_empty() {
                return None;
            }

            let mut rng = thread_rng();
            let total_bias = *cumulative_weights.last().unwrap();
            let random_value: u128 = rng.gen_range(0..total_bias);

            let index = match cumulative_weights.binary_search_by(|&bias| bias.partial_cmp(&random_value).unwrap()) {
                Ok(i) => i,      // Exact match
                Err(i) => i,     // Closest match (next higher value)
            };

            Some((index, nodes[index].clone()))

    }


    /// This function is the defecto way to get a consensor.
    /// When nodeList is first refreshed the round trip http request time for the nodes are
    /// unknown. The function will round robin from the list to return consensor. 
    /// During the interaction with the each consensors in each rpc call, it will collect the round trip time for
    /// each node. The values are then used to calculate a weighted bias for
    /// node selection. Subsequent call will be redirected towards the node based on that bias and round robin
    /// is dismissed.
    pub async fn get_next_appropriate_consensor(&self) -> Option<(usize, Consensor)> {
        match self.list_prepared.load(std::sync::atomic::Ordering::Relaxed) {
            true => {
               self.get_random_consensor_biased().await 
            },
            false => {
                let index = self.round_robin_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                let nodes = self.active_nodelist.read().await;
                if index >= nodes.len() {
                    // dropping the `nodes` is really important here
                    // prepare_list() will acquire write lock
                    // but this scope here has a simultaneous read lock
                    // this will cause a deadlock if not drop
                    drop(nodes);
                    self.prepare_list().await;
                    return self.get_random_consensor_biased().await;
                }
                return Some((index, nodes[index].clone()));
            },
        }
    }

    pub fn set_consensor_trip_ms(&self, node_id: String, trip_ms: u128){
        let trip_ms_map = self.trip_ms.clone();

        tokio::spawn(async move {
            let mut guard = trip_ms_map.write().await;
            guard.insert(node_id, trip_ms);           
            drop(guard);                              
        });        
    }

    pub async fn inject_transaction(&self, tx: serde_json::Value) -> Result<serde_json::Value, serde_json::Value>{
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "tx": tx,
        });

        let (index, consensor) = match self.get_next_appropriate_consensor().await {
            Some((index, consensor)) => (index, consensor),
            None => return Err("Failed to select consensor".into()),
        };

        let start = std::time::Instant::now();
         let resp = client.post(&format!("http://{}:{}/inject", consensor.ip, consensor.port))
            .json(&payload)
            .send()
            .await;
        let duration = start.elapsed().as_millis();

        self.set_consensor_trip_ms(consensor.id, duration);
        
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

        let (index, consensor) = match self.get_next_appropriate_consensor().await {
            Some((index,consensor)) => (index,consensor),
            None => return Err("Failed to select consensor".into()),
        };

        let start = std::time::Instant::now();
        let resp = reqwest::get(&format!("http://{}:{}/account/{}", consensor.ip, consensor.port, address)).await;
        let duration = start.elapsed().as_millis();

        self.set_consensor_trip_ms(consensor.id, duration);

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

        let (index, consensor) = match self.get_random_consensor_biased().await {
            Some((index, consensor)) => (index, consensor),
            None => return Err("Failed to select consensor".into()),
        };

        let start = std::time::Instant::now();
        let resp = reqwest::get(&format!("http://{}:{}/transaction/{}", consensor.ip, consensor.port, id)).await;

        let duration = start.elapsed().as_millis();

        self.set_consensor_trip_ms(consensor.id, duration);

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
        let (index, consensor) = match self.get_next_appropriate_consensor().await {
            Some((index, consensor)) => (index, consensor),
            None => return Err("Failed to select consensor".into()),
        };


        let start = std::time::Instant::now();
        let resp = reqwest::get(&format!("http://{}:{}/messages/{}", consensor.ip, consensor.port, chat_id)).await;

        let duration = start.elapsed().as_millis();

        self.set_consensor_trip_ms(consensor.id, duration);

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

