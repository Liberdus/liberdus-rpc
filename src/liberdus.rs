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
pub struct Consensor{
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub publicKey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub load: Option<NodeLoad>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[derive(Clone)]
pub struct NodeLoad{
    pub external: f64,
    pub internal: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ConsensorLoadResp{
    pub load: f64,
    pub nodeLoad: NodeLoad,
}

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
    current: u64,
    crypto: Arc<crypto::ShardusCrypto>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct GetAccountResp{
    account: serde_json::Value,
}

impl  Liberdus {
    pub fn new (sc: Arc<crypto::ShardusCrypto>, archivers: Arc<RwLock<Vec<archivers::Archiver>>>) -> Self{
        Liberdus{
            current: 0,
            active_nodelist: Arc::new(RwLock::new(Vec::new())),
            archivers,
            crypto: sc,
        }
    }

    pub async fn update_active_nodelist(self: Arc<Self>){
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Vec<Consensor>, std::io::Error>>(64);

        let static_self = self.clone();
        tokio::spawn(async move {
                let mut tmp: Vec<Consensor> = Vec::new();
                while let Some(result) = rx.recv().await{
                    if result.is_ok(){
                        tmp.extend(result.unwrap());
                    }
                };
                tmp.dedup_by(|a,b| a.id == b.id);
                static_self.sort_consensors_by_load(tmp).await;

        });
        for archiver in self.archivers.read().await.iter(){
            
            let url = format!("http://{}:{}/full-nodelist?activeOnly=true", archiver.ip, archiver.port);
            let transmitter = tx.clone();
            let static_self = self.clone();

            tokio::spawn(async move {

                let tx_payload = match reqwest::get(&url).await {
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
                let _ = transmitter.send(tx_payload).await;
                drop(transmitter);

            });
        }

    }

    async fn sort_consensors_by_load(self: Arc<Self>, consensors: Vec<Consensor>) {
        let long_lived_self = self.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Consensor, std::io::Error>>(64);
    
        tokio::spawn(async move {
            let mut tmp: Vec<Consensor> = Vec::new();
            while let Some(result) = rx.recv().await{
                if result.is_ok(){
                    tmp.push(result.unwrap());
                }
            }
    
            // lowest load score first
            let fallback_bad_load = NodeLoad{
                external: 1.0,
                internal: 1.0,
            };
            tmp.sort_by(|a, b| {
                let a = a.load.as_ref().unwrap_or(&fallback_bad_load);
                let b = b.load.as_ref().unwrap_or(&fallback_bad_load);
                let a_score = a.external + a.internal;
                let b_score = b.external + b.internal;
                a_score.partial_cmp(&b_score).unwrap_or(Ordering::Equal)
            });
            let mut guard = long_lived_self.active_nodelist.write().await;
            *guard = tmp;
            drop(guard);
        });
        for node in consensors.iter() {
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
                                };
                               
                                Ok(tmp)
    
                            },
                            Err(e) => {
                                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Malfromed Load Object"))
                            },
                        }
                    },
                    Err(e) => {
                        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Bad Request {}:{}", node.ip, node.port)))
                    }
                };
    
                let _ = transmitter.send(resp).await;
                drop(transmitter);
            });
        }
    
    }
    
    pub async fn select_random_consensor(&self) -> Option<Consensor>{
        let node = self.active_nodelist.read().await;
        if node.len() == 0{
            return None
        }
        let index = rand::thread_rng().gen_range(0..node.len());
        Some(node[index].clone())
    }


    pub async fn inject_transaction(&self, tx: serde_json::Value) -> Result<serde_json::Value, serde_json::Value>{
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "tx": tx,
        });

        let consensor = match self.select_random_consensor().await {
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

        let consensor = match self.select_random_consensor().await {
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

