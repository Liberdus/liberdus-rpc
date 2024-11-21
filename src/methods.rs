use crate::{archivers::{select_random_archiver, Archiver}, liberdus, rpc};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;


pub async fn lib_send_transaction(req: rpc::RpcRequest, archivers: Arc<RwLock<Vec<Archiver>>>) -> rpc::RpcResponse  {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() > 0 => {
            let tx = values[0].clone();
            let archiver = select_random_archiver(archivers).await;
            match archiver {
                Some(archiver) => {
                    let consensor = liberdus::fast_select_random_consensor(archiver).await;
                    match consensor {
                        Some(consensor) => {
                            match liberdus::inject_transaction(consensor, tx).await {
                                Ok(resp) => rpc::generate_success_response(req.id, resp),
                                Err(e) => rpc::generate_error_response(req.id, e.to_string().into()),
                            }
                        }
                        None => rpc::generate_error_response(req.id, "Failed to select consensor".into()),
                    }
                }
                None => rpc::generate_error_response(req.id, "Failed to select archiver".into()),
            }




        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into()),
    }
}

pub async fn lib_get_transaction_receipt(req: rpc::RpcRequest) -> rpc::RpcResponse {
    todo!()
}
