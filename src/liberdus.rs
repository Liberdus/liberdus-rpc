use crate::archivers;
use reqwest;
use serde_json;
use serde;
use rand::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::crypto;


#[derive(serde::Deserialize, serde::Serialize)]
#[derive(Clone)]
pub struct Consensor{
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub publicKey: String,
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

    pub async fn populate_active_nodelist(&self){
        for i in 0..self.archivers.read().await.len(){
            let archiver = self.archivers.read().await[i].clone();
            match reqwest::get(&format!("http://{}:{}/full-nodelist?activeOnly=true", archiver.ip, archiver.port)).await {
                Ok(resp) => {
                    let body: Result<SignedNodeListResp, _> = serde_json::from_str(&resp.text().await.unwrap());
                    match body{
                        Ok(body) => {
                            if !self.verify_signature(&body){
                                continue
                            }
                            self.active_nodelist.write().await.extend(body.nodeList);
                            continue
                        },
                        Err(_) => continue,
                    }
                },
                Err(_) => continue,
            }
        }

        let mut guard = self.active_nodelist.write().await;
        guard.dedup_by(|a, b| a.id == b.id);
        drop(guard);

    }
    
    pub async fn select_random_consensor(&self) -> Option<Consensor>{
        let guard = self.active_nodelist.read().await;
        if guard.len() == 0{
            return None
        }
        let index = rand::thread_rng().gen_range(0..guard.len());
        Some(guard[index].clone())
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

