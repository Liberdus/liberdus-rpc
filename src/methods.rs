use crate::{liberdus, rpc::{self, RpcRequest}};
use serde_json::Value;
use std::sync::Arc;
use rand::prelude::*;


pub async fn lib_send_transaction(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse  {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() > 0 => {
            let tx = values[0].clone();
            // match liberdus.inject_transaction(tx).await {
            //     Ok(result) => return rpc::generate_success_response(req.id, result),
            //     Err(e) => return rpc::generate_error_response(req.id, e.to_string().into()),
            // };
            let max_retry = {
                let mut rng = rand::thread_rng();
                rng.gen_range(3..5)
            };
            let mut counter = 0;
            loop {
                match liberdus.inject_transaction(tx.clone()).await {
                    Ok(result) => {
                        let parsed_result: liberdus::TxInjectRespInner = serde_json::from_value(result.clone()).unwrap();
                        if parsed_result.success {
                            return rpc::generate_success_response(req.id, result);
                        }
                        else {
                            if counter < max_retry {
                                counter += 1;
                                continue;
                            }
                            else {
                                return rpc::generate_error_response(req.id, parsed_result.reason.into(), -32600);
                            }
                        }
                    },
                    Err(e) => {
                        if counter < max_retry {
                            counter += 1;
                            continue;
                        }
                        else {
                            return rpc::generate_error_response(req.id, e.to_string().into(), -32600);
                        }
                    },
                }
            }


        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

pub async fn lib_get_transaction_receipt(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() > 0 => {
            let tx_hash = values[0].as_str().unwrap().to_string();
            match liberdus.get_transaction_receipt(&tx_hash).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

pub async fn lib_get_transaction_history(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let account_id = values[0].as_str().unwrap_or("").to_string();
            match liberdus.get_transaction_history(&account_id).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

pub async fn lib_get_account(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let addr = values[0].as_str().unwrap().to_string();
            match liberdus.get_account_by_addr(&addr).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

pub async fn lib_get_messages(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let chat_id = values[0].as_str().unwrap_or("").to_string();
            match liberdus.get_messages(&chat_id).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}


pub async fn lib_subscribe(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>, transmitter: Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>, subscription_id: Option<String>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    if transmitter.is_none() {
        return rpc::generate_error_response(req.id, "Use Websocket".into(), -32600);
    }

    if subscription_id.is_none() {
        return rpc::generate_error_response(req.id, "Invalid subscription id".into(), -32600);
    }

    let sub_id = match subscription_id {
        Some(sub_id) => sub_id,
        _ => return rpc::generate_error_response(req.id, "Invalid subscription id".into(), -32600),
    };


    match params {
        Value::Array(values) if values.len() > 0 => {
            let chat_id = values[0].as_str().unwrap_or("").to_string();
            
            liberdus.subscribe_chat_room(&chat_id, &sub_id.clone(), transmitter.unwrap()).await;

            rpc::generate_success_response(req.id, sub_id.clone().into())

        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}


pub async fn lib_unsubscribe(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let sub_id = values[0].as_str().unwrap_or("").to_string();
            
            liberdus.unsubscribe_chat_room(&sub_id).await;

            rpc::generate_success_response(req.id, serde_json::Value::Bool(true))

        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}
