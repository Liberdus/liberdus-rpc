use serde_json;
use reqwest;
use serde;

#[derive(serde::Deserialize)]
struct TxResp {
    #[serde(skip_serializing_if = "Option::is_none")]
    success: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    transactions: Vec<Transaction>,
}

#[derive(serde::Deserialize)]
struct Transaction {
    #[allow(non_snake_case)]
    originalTxData: serde_json::Value,
}

pub async fn get_transaction(collector_ip: &String, collector_port: &u16, id: &String) -> Option<serde_json::Value> {
    let built_url = format!("http://{}:{}/api/transaction?txId={}", collector_ip, collector_port, id);
    let resp = match reqwest::get(built_url).await {
        Ok(resp) => resp,
        Err(_) => { return None; },
    };

    let result: Option<TxResp> = match resp.status() {
        reqwest::StatusCode::OK => {
            let json = match resp.json().await {
                Ok(json) => json,
                Err(_) => { return None; },
            };

            json
        },
        _ => None,
    };

   return match result {
        Some(result) => {
            if result.success? && result.transactions.len() > 0 {
                Some(result.transactions[0].originalTxData.clone())
            } else {
                None
            }
        },
        None => None,
    };


    
}


pub async fn get_message() {
    todo!()
}

