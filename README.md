# Liberdus RPC
Liberdus RPC is a multi threaded server that route traffic between clients and liberdus network. The primary job for the rpc is to route inject transaction the consensor node in the network that has relatively low loads. Consensor node are sorted lowest load to highest node in the rpc and has weighted random picking algorithm to select the low load node to inject the transaction. This rpc has its own distant implementation of @shardus/archive-discovery to discover archive nodes and then collect consensus node.

# Planned
- [ ] Transaction receipt are offloaded to the archive node and rpc should obtain the receipt from the archive node via offchain data distributor services as a first source and fallback to asking consensor node. 

# Setup on Linux
- Install Rust  
  As a regular user run  
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

- Download Liberdus RPC  
  mkdir rpc  
  cd rpc  
  wget https://github.com/Liberdus/liberdus-rpc/archive/refs/heads/main.zip  
  unzip main.zip  
  cd liberdus-rpc-main

- Build RPC  
  cargo build  
  If you run into errors about missing packages install them. For example: sudo apt install libssl-dev pkg-config  

- Configure RPC  
  Should look something like this:  
```
src/archiver_seed.json
[{"publicKey":"758b1c119412298802cd28dbfa394cdfeecc4074492d60844cc192d632d84de3","port":4000,"ip":"63.141.233.178"}]

src/config.json
{
    "rpc_http_port": 8545,
    "archiver_seed_path": "./src/archiver_seed.json",
    "nodelist_refresh_interval_sec": 40,
    "max_http_timeout_ms": 4000,
    "debug": true,
    "collector": {
        "port": 6101,
        "ip": "0.0.0.0"
    },
    "standalone_network": {
        "enabled":  true,
        "replacement_ip": "63.141.233.178"
    }
}
```

- Run RPC
  ./target/debug/liberdus-rpc  

- Verify RPC


---


