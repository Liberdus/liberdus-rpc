use crate::archivers;
use reqwest;
use serde_json;
use serde;
use rand::prelude::*;


#[derive(serde::Deserialize, serde::Serialize)]
pub struct Consensor{
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub publicKey: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct NodeListResp{
    nodeList: Vec<Consensor>,
    sign: archivers::Signature,
}


pub async fn fast_select_random_consensor(archiver: archivers::Archiver) -> Option<Consensor>{
    match reqwest::get(&format!("http://{}:{}/full-nodelist?activeOnly=true", archiver.ip, archiver.port)).await {
        Ok(resp) => {
            let body: Result<NodeListResp, _> = serde_json::from_str(&resp.text().await.unwrap());
            match body{
                Ok(body) => {
                    let random_index = rand::thread_rng().gen_range(0..body.nodeList.len());
                    let winner = Consensor{
                        id: body.nodeList[random_index].publicKey.clone(),
                        ip: body.nodeList[random_index].ip.clone(),
                        port: body.nodeList[random_index].port,
                        publicKey: body.nodeList[random_index].publicKey.clone(),
                    };
                    Some(winner)
                },
                Err(_) => None,
            }
        },
        Err(_) => None,
    }

}


pub async fn inject_transaction(consensor: Consensor, tx: serde_json::Value) -> Result<serde_json::Value, reqwest::Error>{
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "tx": tx,
    });

     let resp = client.post(&format!("http://{}:{}/inject", consensor.ip, consensor.port))
        .json(&payload)
        .send()
        .await;
    
    match resp{
        Ok(resp) => {
            let body: serde_json::Value = resp.json().await?;
            Ok(body)
        },
        Err(e) => Err(e),
    }
}
