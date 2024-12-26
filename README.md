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

- Build RPC
- Configure RPC
- Run RPC
- Verify RPC


---


