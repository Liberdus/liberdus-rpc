use crate::{liberdus, rpc};
use serde_json::Value;
use std::sync::Arc;


pub async fn lib_send_transaction(req: rpc::RpcRequest, liberdus: Arc<liberdus::Liberdus>) -> rpc::RpcResponse  {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() > 0 => {
            let tx = values[0].clone();
            match liberdus.inject_transaction(tx).await {
                Ok(result) => return rpc:: generate_success_response(req.id, result),
                Err(e) => return rpc::generate_error_response(req.id, e.to_string().into()),
            };

        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into()),
    }
}

pub async fn lib_get_transaction_receipt(req: rpc::RpcRequest) -> rpc::RpcResponse {
    todo!()
}

pub async fn lib_get_account(req: rpc::RpcRequest, liberdus: Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into()),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let addr = values[0].as_str().unwrap().to_string();
            match liberdus.get_account_by_addr(addr).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into()),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into()),
    }
}
