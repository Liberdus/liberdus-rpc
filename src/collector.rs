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

pub async fn get_transaction_history(collector_ip: &String, collector_port: &u16, account_id: &String) -> Result<serde_json::Value, String> {
    let built_url = format!("http://{}:{}/api/transaction?accountId={}", collector_ip, collector_port, account_id);
    let resp = match reqwest::get(built_url).await {
        Ok(resp) => resp,
        Err(e) => return Err(e.to_string()),
    };

    let result: TxResp = match resp.status() {
        reqwest::StatusCode::OK => {
            match resp.json().await {
                Ok(json) => json,
                Err(e) => return Err(e.to_string()),
            }
        },
        status => return Err(format!("HTTP error: {}", status)),
    };

    if result.success.unwrap_or(false) {
        let transactions: Vec<serde_json::Value> = result.transactions
            .into_iter()
            .map(|tx| tx.originalTxData)
            .collect();
        Ok(serde_json::json!({ "transactions": transactions }))
    } else {
        Err(result.error.unwrap_or_else(|| "Unknown error".to_string()))
    }
}

pub async fn get_message() {
    todo!()
}
