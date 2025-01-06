//! This module contains the node management logic require for load balancing with consensor nodes
use crate::{archivers, collector, config};
use reqwest;
use serde_json;
use serde;
use rand::prelude::*;
use std::{cmp::Ordering, collections::HashMap, sync::{atomic::{AtomicBool, AtomicUsize}, Arc}};
use tokio::sync::RwLock;
use crate::crypto;
use std::collections::HashSet;
use crate::rpc;



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
    pub rng_bias: Option<f64>,
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
pub struct TxInjectResp{
    pub result: Option<TxInjectRespInner>,
    pub error: Option<serde_json::Value>
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct TxInjectRespInner{
    pub reason: String,
    pub status: u32,
    pub success: bool,
    #[allow(non_snake_case)]
    pub txId: Option<String>,
}

pub struct Liberdus {
    pub active_nodelist: Arc<RwLock<Vec<Consensor>>>,
    trip_ms: Arc<RwLock<HashMap<String, u128>>>,
    archivers: Arc<RwLock<Vec<archivers::Archiver>>>,
    round_robin_index: Arc<AtomicUsize>,
    list_prepared: Arc<AtomicBool>,
    crypto: Arc<crypto::ShardusCrypto>,
    load_distribution_commulative_bias: Arc<RwLock<Vec<f64>>>,
    pub chat_room_subscriptions: Arc<RwLock<ChatRoomSubscriptionData>>,
    config: Arc<config::Config>,
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

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ChatAccount {
    hash: String,
    id: String,
    messages: Vec<serde_json::Value>,
    timestamp: u128,
    #[serde(rename = "type")] // Map JSON "type" to the Rust field "type_field"
    type_field: String,
}

impl  Liberdus {
    pub fn new (sc: Arc<crypto::ShardusCrypto>, archivers: Arc<RwLock<Vec<archivers::Archiver>>>, config: config::Config) -> Self{
        Liberdus{
            config: Arc::new(config),
            round_robin_index: Arc::new(AtomicUsize::new(0)),
            trip_ms: Arc::new(RwLock::new(HashMap::new())),
            active_nodelist: Arc::new(RwLock::new(Vec::new())),
            list_prepared: Arc::new(AtomicBool::new(false)),
            archivers,
            crypto: sc,
            load_distribution_commulative_bias: Arc::new(RwLock::new(Vec::new())),
            chat_room_subscriptions: Arc::new(RwLock::new(ChatRoomSubscriptionData{
                subscriptions: HashMap::new(),
                subscribed_chats: HashMap::new(),
                chat_room_by_sub_id: HashMap::new(),
                last_chat_states: HashMap::new(),
            })),
        }


    }

    pub async fn update_active_nodelist(&self){

        let archivers = self.archivers.read().await;

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
                Ok(mut nodelist) => {
                    if self.config.standalone_network.enabled {
                        let replacement_ip = self.config.standalone_network.replacement_ip.clone();
                        for node in nodelist.iter_mut(){
                            node.ip = replacement_ip.clone();
                        }
                    }

                    {
                       let mut guard = self.active_nodelist.write().await; 
                       *guard = nodelist;
                    }

                   self.round_robin_index.store(0, std::sync::atomic::Ordering::Relaxed);
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

    /// Calculates a node's bias for weighted random selection based on its HTTP round-trip time (RTT).
    /// 
    /// # Formula
    /// The function uses the following formula to calculate bias:
    /// ```math
    /// bias = 1.0 - (timetaken_ms - min_timeout) / (max_timeout - min_timeout)
    /// ```
    /// where:
    /// - `timetaken_ms` is the round-trip time (RTT) of the node in milliseconds.
    /// - `min_timeout` is the theoretical minimum RTT (set to 0.01 ms for numerical stability).
    /// - `max_timeout` is the maximum allowable RTT.
    /// 
    /// # Explanation
    /// The bias is normalized to a scale between 0 and 1, where:
    /// - A lower `timetaken_ms` (faster node) results in a higher bias, making the node more likely to be selected.
    /// - A higher `timetaken_ms` (slower node) results in a lower bias, making the node less likely to be selected.
    /// 
    /// ## Steps:
    /// 1. **Normalize RTT:** The RTT (`timetaken_ms`) is normalized using the formula:
    ///    ```math
    ///    normalized_rtt = (timetaken_ms - min_timeout) / (max_timeout - min_timeout)
    ///    ```
    ///    This maps the RTT to a range between 0 (minimum RTT) and 1 (maximum RTT).
    /// 2. **Invert the Normalization:** The bias is calculated as:
    ///    ```math
    ///    bias = 1.0 - normalized_rtt
    ///    ```
    ///    This ensures that faster nodes (lower RTT) have a higher bias (closer to 1.0),
    ///    while slower nodes (higher RTT) have a lower bias (closer to 0.0).
    /// 
    /// ## Special Case
    /// If `max_timeout` is `1` (to prevent division by zero), the function assumes all nodes are equally performant and returns a bias of `1.0`.
    /// 
    /// ## Example
    /// For a node with:
    /// - `timetaken_ms = 100`
    /// - `max_timeout = 500`
    /// The bias is calculated as:
    /// ```math
    /// normalized_rtt = (100 - 0.01) / (500 - 0.01) ≈ 0.19996
    /// bias = 1.0 - 0.19996 ≈ 0.80004
    /// ```
    /// This node is more likely to be selected compared to nodes with higher RTTs.
    /// 
    /// # Why This Works
    /// - This bias calculation ensures that faster nodes (lower RTTs) are favored during selection.
    /// - Nodes with RTTs close to `max_timeout` are effectively penalized, reducing their likelihood of being selected.
    /// - The linear normalization and inversion provide a smooth, predictable weighting system.
    fn calculate_bias(&self, timetaken_ms: u128, max_timeout: u128) -> f64 {
        if max_timeout == 1 {
            return 1.0; // All timeouts are the same
        }
        let timetaken_ms_f = timetaken_ms as f64;
        let min_timeout_f = 0.0 as f64;
        let max_timeout_f = max_timeout as f64;
        1.0 - (timetaken_ms_f - min_timeout_f) / (max_timeout_f - min_timeout_f)
    }

    /// Computes and maintains a cumulative bias distribution for node selection.
    /// 
    /// # What is Cumulative Bias?
    /// Cumulative bias is a method to efficiently perform weighted random selection.
    /// Each node's individual bias is calculated based on its HTTP round-trip time (RTT),
    /// and these biases are accumulated into a cumulative distribution. This allows for
    /// efficient random selection of nodes where the probability of selection is proportional
    /// to their bias.
    /// 
    /// # How It Works
    /// 1. **Calculate Individual Bias:** For each node, an individual bias is computed using
    ///    the `calculate_bias` function, which maps RTT to a value between `0.0` (poor performance)
    ///    and `1.0` (high performance).
    /// 2. **Build Cumulative Distribution:** The cumulative bias is computed by summing
    ///    the biases iteratively. The resulting vector represents a range of cumulative weights:
    ///    ```math
    ///    cumulative_bias[i] = bias[0] + bias[1] + ... + bias[i]
    ///    ```
    /// 3. **Select Nodes:** When selecting a node, a random value is generated in the range
    ///    `[0, total_bias]`, where `total_bias` is the last element of the cumulative vector.
    ///    The appropriate node is determined using a binary search over the cumulative biases.
    /// 
    /// # Why It Works
    /// - The cumulative bias ensures that nodes with higher individual biases have a larger
    ///   range of values in the cumulative distribution, making them more likely to be selected.
    /// - The binary search provides efficient lookup for random selection, making this approach
    ///   scalable for large node lists.
    /// 
    /// # Example
    /// Given the following nodes and biases:
    /// ```text
    /// Node  Bias
    /// A     0.2
    /// B     0.5
    /// C     0.3
    /// ```
    /// The cumulative bias array will be:
    /// ```text
    /// [0.2, 0.7, 1.0]
    /// ```
    /// A random value between `0.0` and `1.0` is generated:
    /// - A value in `[0.0, 0.2)` selects Node A.
    /// - A value in `[0.2, 0.7)` selects Node B.
    /// - A value in `[0.7, 1.0]` selects Node C.
    /// 
    /// # Efficiency
    /// - Cumulative bias computation is O(n), where `n` is the number of nodes.
    /// - Random selection using binary search is O(log n).
    /// 
    /// # Implementation in `prepare_list`
    /// - The cumulative bias is stored in `self.load_distribution_commulative_bias`.
    /// - It is recalculated whenever the node list is updated or RTT data changes.
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

        let max_timeout = self.config.max_http_timeout_ms.try_into().unwrap_or(4000); // 3 seconds
        let mut sorted_nodes = nodes;

        sorted_nodes.sort_by(|a, b| {
            let a_time = trip_ms.get(&a.id).unwrap_or(&max_timeout);
            let b_time = trip_ms.get(&b.id).unwrap_or(&max_timeout);
            a_time.cmp(b_time)
        });

        let mut total_bias = 0.0;
        let mut cumulative_bias = Vec::new();
        for node in &mut sorted_nodes {
            let last_http_round_trip = *trip_ms.get(&node.id).unwrap_or(&max_timeout);
            let bias = self.calculate_bias(last_http_round_trip, max_timeout);
            node.rng_bias =  Some(bias);
            total_bias += bias;
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
            let total_bias = *cumulative_weights.last().unwrap_or(&1.0);


            // If all nodes have the same bias, return a random node
            if total_bias == 0.0 {
                let idx = rng.gen_range(0..nodes.len());
                self.round_robin_index.store(0, std::sync::atomic::Ordering::Relaxed);
                self.list_prepared.store(false, std::sync::atomic::Ordering::Relaxed);
                return Some((idx, nodes[idx].clone()));
            }

            let random_value: f64 = rng.gen_range(0.0..total_bias);

            let index = match cumulative_weights.binary_search_by(
                |&bias| bias.partial_cmp(&random_value).unwrap_or(Ordering::Equal)
            ) {
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
        if self.active_nodelist.read().await.is_empty() {
            return None;
        }
        match 
            self.list_prepared.load(std::sync::atomic::Ordering::Relaxed) && 
            (self.load_distribution_commulative_bias.read().await.clone().len() == self.active_nodelist.read().await.len()) {
            true => {
               return self.get_random_consensor_biased().await 
            },
            false => {
                let index = self.round_robin_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed).clone();

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
        // list already prepared on the first round robin,  no need to keep recording rtt for nodes
        if self.list_prepared.load(std::sync::atomic::Ordering::Relaxed) == true {
            return;
        }

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

        let (_index, consensor) = match self.get_next_appropriate_consensor().await {
            Some((index, consensor)) => (index, consensor),
            None => return Err("Failed to select consensor".into()),
        };

        let start = std::time::Instant::now();
         let resp = client.post(&format!("http://{}:{}/inject", consensor.ip, consensor.port))
            .json(&payload)
            .send()
            .await;
        let duration = start.elapsed().as_millis();
        self.set_consensor_trip_ms(consensor.id.clone(), duration);
        
        match resp{
            Ok(resp) => {
                let body: TxInjectResp = resp.json().await.unwrap();
                if let Some(result) = body.result{
                    Ok(serde_json::to_value(result).unwrap())
                }
                else{
                    Err(body.error.unwrap())
                }
            },

            Err(e) => Err(e.to_string().into()),
        }
    }

    pub async fn get_account_by_addr(&self, address: &String) -> Result<serde_json::Value, serde_json::Value>{

        let (_index, consensor) = match self.get_next_appropriate_consensor().await {
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

    pub async fn get_transaction_receipt(&self, id: &String) -> Result<serde_json::Value, serde_json::Value>{
        
        let receipt  = collector::get_transaction(&self.config.collector.ip, &self.config.collector.port, &id).await;

        if let Some(receipt) = receipt {
            return Ok(receipt)
        }

        
        let (_index, consensor) = match self.get_next_appropriate_consensor().await {
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

    pub async fn get_transaction_history(&self, account_id: &String) -> Result<serde_json::Value, serde_json::Value> {        
        match collector::get_transaction_history(&self.config.collector.ip, &self.config.collector.port, &account_id).await {
            Ok(result) => Ok(result),
            Err(e) => Err(e.to_string().into()),
        }
    }

    pub async fn get_messages(&self, chat_id: &String) -> Result<serde_json::Value, serde_json::Value>{
        let (_index, consensor) = match self.get_next_appropriate_consensor().await {
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

    pub async fn subscribe_chat_room(&self, chat_id: &String, sub_id: &String, sender: tokio::sync::mpsc::UnboundedSender<serde_json::Value>){

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let mut guard = self.chat_room_subscriptions.write().await;
        guard.subscriptions.insert(sub_id.clone(), sender);
        guard.subscribed_chats.entry(chat_id.clone()).or_insert(HashSet::new()).insert(sub_id.clone());
        guard.chat_room_by_sub_id.insert(sub_id.clone(), chat_id.clone());
        guard.last_chat_states.insert(chat_id.clone(), (now, 0));

    }

    pub async fn unsubscribe_chat_room(&self, sub_id: &String){
        let mut guard = self.chat_room_subscriptions.write().await;
        guard.subscriptions.remove(sub_id);
        let chat_id = guard.chat_room_by_sub_id.remove(sub_id).unwrap();
        guard.subscribed_chats.get_mut(&chat_id).unwrap().remove(sub_id);
        guard.chat_room_by_sub_id.remove(sub_id);
        guard.last_chat_states.remove(&chat_id);

    }

    pub async fn get_last_chat_state(&self, chat_id: &String) -> (u128, usize){
        let guard = self.chat_room_subscriptions.read().await;
        *guard.last_chat_states.get(chat_id).unwrap_or(&(0, 0))
    }

    pub async fn get_subscriber(&self, sub_id: &String) -> Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>{
        let guard = self.chat_room_subscriptions.read().await;
        guard.subscriptions.get(sub_id).cloned()
    }

    pub async fn get_chat_room_subscriptions(&self, chat_id: &String) -> Option<HashSet<String>>{
        let guard = self.chat_room_subscriptions.read().await;
        guard.subscribed_chats.get(chat_id).cloned()
    }

    /// Discovers new chat messages for all subscribed chat rooms and sends updates to the corresponding subscribers.
    /// 
    /// This function operates as follows:
    /// 1. **Check Active Subscriptions:** If there are no active subscriptions, the function immediately exits.
    /// 2. **Setup Communication Channel:** Creates an unbounded channel to handle message fetching and delivery.
    /// 3. **Spawn Listener Task:** A new asynchronous task is spawned to process the messages received from the chat rooms:
    ///     - **Message Parsing:** Each message is deserialized into a `ChatAccount` struct.
    ///     - **State Validation:** Compares the last known state (timestamp and message index) with the new state of the chat room.
    ///         - If the new state is older than or equal to the last known state, the message is skipped.
    ///         - Otherwise, determines how many new messages are present.
    ///     - **Broadcast Messages:** For each subscriber of the chat room:
    ///         - Constructs a payload with the subscription ID and new messages.
    ///         - Sends the payload to the WebSocket connection associated with the subscription.
    ///         - Tracks disconnected subscribers for cleanup.
    ///     - **Update State:** Updates the last known state for the chat room after broadcasting the messages.
    /// 4. **Fetch Chat Room Data:** Iterates through all subscribed chat room addresses and spawns tasks to fetch the latest
    ///    data for each chat room from the blockchain or backend storage:
    ///     - Calls `get_account_by_addr` for each chat room to retrieve the latest messages.
    ///     - Sends the results to the communication channel for processing.
    /// 
    /// ## Considerations:
    /// - **Concurrency:** The function uses asynchronous tasks to handle multiple chat rooms concurrently.
    /// - **Fault Tolerance:** Handles errors gracefully by skipping over invalid or failed message deliveries and 
    ///   unsubscribing disconnected clients.
    /// - **Efficiency:** By comparing timestamps and message indices, the function avoids re-sending old messages, ensuring
    ///   efficient use of network and compute resources.
    /// 
    /// ## Example Workflow:
    /// - **Initial State:**
    ///     - Chat room `chat1` has 10 messages.
    ///     - Subscriber `sub1` is subscribed to `chat1`.
    /// - **New Messages Arrive:**
    ///     - `chat1` now has 15 messages.
    ///     - The function detects 5 new messages and sends them to `sub1`.
    /// - **Subscriber Disconnects:**
    ///     - If `sub1` disconnects, the function removes the subscription and cleans up the internal state.
    pub async fn discover_new_chats(self: Arc<Self>){
        if self.chat_room_subscriptions.read().await.subscriptions.len() <= 0 {
            return;
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<serde_json::Value, serde_json::Value>>();

        let long_lived_self = Arc::clone(&self);
        tokio::spawn(async move {
            let mut bad_subscriptions = Vec::new();
            let mut new_chat_states = HashMap::new();
            while let Some(msg) = rx.recv().await {
                match msg{
                    Ok(msg) => {
                        let resp: ChatAccount = serde_json::from_value(msg).unwrap();
                        let new_timestamp = resp.timestamp;

                        let (timestamp, index) = long_lived_self.get_last_chat_state(&resp.id).await;
                        let indices_to_pop_from_the_back = resp.messages.len() - index;

                        if timestamp > new_timestamp {
                            continue;
                        }

                        if indices_to_pop_from_the_back <= 0 {
                            continue;
                        }

                        let subs = long_lived_self.get_chat_room_subscriptions(&resp.id).await.unwrap_or(HashSet::new());

                        for sub in subs{
                            let ws_sink_transmitter = long_lived_self.get_subscriber(&sub).await.unwrap();

                            let payload = serde_json::json!({
                                "subscription_id": sub,
                                "new_message": resp.messages.clone().split_off(resp.messages.len() - indices_to_pop_from_the_back),
                            });

                            let rpc_resp = rpc::generate_success_response(1, payload);

                            let json = serde_json::json!({
                                "jsonrpc": rpc_resp.jsonrpc,
                                "result": rpc_resp.result,
                                "error": rpc_resp.error,
                                "id": rpc_resp.id,
                            });

                            let client_disconnected = match ws_sink_transmitter.send(json) {
                                Ok(_) => {
                                   new_chat_states.insert(resp.id.clone(), (new_timestamp, resp.messages.len()));
                                   false
                                },
                                Err(_e) => {
                                    true
                                }
                            };

                            if client_disconnected{
                                drop(ws_sink_transmitter);
                                bad_subscriptions.push(sub);
                            }
                        }


                    },
                    Err(e) => {
                        println!("Error: {}", e);
                    }
                }

            }
            for sub in bad_subscriptions{
                long_lived_self.unsubscribe_chat_room(&sub).await;
            }
            for (chat_id, (timestamp, index)) in new_chat_states{
                let mut guard = long_lived_self.chat_room_subscriptions.write().await;
                guard.last_chat_states.insert(chat_id, (timestamp, index));
                drop(guard);
            }
        });

        let chat_room_addresses = self.chat_room_subscriptions.read().await
                                        .subscribed_chats.keys().cloned().collect::<Vec<String>>();

        for chat_room_address in chat_room_addresses{
            let tx_clone = tx.clone();
            let self_cloned = Arc::clone(&self);
            tokio::spawn(async move {
                let resp = self_cloned.get_account_by_addr(&chat_room_address).await;
                let _ = tx_clone.send(resp);
            });
        }

    }



}

type SubscriptionId = String;
type ChatRoomAddress = String;
pub struct ChatRoomSubscriptionData{
   subscriptions: HashMap<SubscriptionId, tokio::sync::mpsc::UnboundedSender<serde_json::Value>>, 
   subscribed_chats: HashMap<ChatRoomAddress, HashSet<SubscriptionId>>,
   chat_room_by_sub_id: HashMap<SubscriptionId, ChatRoomAddress>,
   last_chat_states: HashMap<ChatRoomAddress, (u128, usize)>,
}



// write tests
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_weighted_random() {
        let mut nodes: Vec<Consensor> = Vec::new();

        let config = config::Config::load().unwrap();

        let _mock_archiver = archivers::Archiver{
            ip: "0.0.0.0".to_string(),
            port: 0,
            publicKey: "0x0".to_string(),
        };

        let archivers = Arc::new(RwLock::new(vec![_mock_archiver]));
        let liberdus = Liberdus::new(Arc::new(crypto::ShardusCrypto::new("69fa4195670576c0160d660c3be36556ff8d504725be8a59b5a96509e0c994bc")), archivers, config);

        for i in 0..500 {
            let node = Consensor {
                publicKey: "0x0".to_string(),
                id: i.to_string(),
                ip: "0.0.0.0".to_string(),
                port: i,
                rng_bias: None,
            };
            nodes.push(node);
        };

        liberdus.active_nodelist.write().await.extend(nodes);

        liberdus.round_robin_index.store(1999, std::sync::atomic::Ordering::Relaxed);

        for i in 0..500 {
            //this artificially sets the round trip time for each node
            // specifically making lower indices have lower round trip time
            liberdus.set_consensor_trip_ms(i.to_string(), (i * 10).min(liberdus.config.max_http_timeout_ms));
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        liberdus.prepare_list().await;

        println!("Cumulative weight {}", liberdus.load_distribution_commulative_bias.read().await.last().unwrap());

        for _i in 0..3000 {
            let (index, _) = liberdus.get_random_consensor_biased().await.unwrap();
            println!("{}", index);
            assert_eq!(true, true);
        }

    }
}



