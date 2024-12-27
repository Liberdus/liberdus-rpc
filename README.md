# Liberdus RPC

Liberdus RPC is a multi-threaded server that routes traffic between clients and the Liberdus network. Its primary role is to facilitate the injection of transactions to a consensor node within the network, ensuring that transactions are sent to nodes with relatively low loads. Consensor nodes are sorted by their load, from lowest to highest, and a weighted random picking algorithm is used to select a low-load node for transaction injection. The RPC server also includes its own implementation of a node discovery mechanism to locate archiver nodes and collect consensor nodes.

## Features

- **Load-based Node Selection**: Consensors are sorted by load and picked using a weighted random selection algorithm.
- **Node Discovery**: Implements a custom node discovery mechanism to find archiver and consensor nodes.
- **Standalone Network Support**: Allows configuration for test networks or scenarios where nodes are on the same machine.

---

## Setup Instructions

### 1. Install Rust
Install Rust by running the following command as a regular user:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Follow the on-screen instructions to complete the installation. 

If Rust is already installed, ensure it is up-to-date:
```bash
rustup update
```

### 2. Download Liberdus RPC
Clone the repository and navigate to the project directory:
```bash
git clone https://github.com/Liberdus/liberdus-rpc.git
cd liberdus-rpc
```

### 3. Build the RPC Server
Run the following command to build the RPC server:
```bash
cargo build
```
If you encounter errors about missing packages, install the necessary dependencies. For example:
```bash
sudo apt install libssl-dev pkg-config
```

### 4. Configure the RPC Server
Create and configure the necessary files for the RPC server. Below is an example configuration:

#### Archiver Seed Configuration (`src/archiver_seed.json`)
This file should contain a list of seed archivers. These are entry points for discovering the network.

```json
[
    {
        "publicKey": "758b1c119412298802cd28dbfa394cdfeecc4074492d60844cc192d632d84de3",
        "port": 4000,
        "ip": "63.141.233.178"
    }
]
```

#### RPC Configuration (`src/config.json`)
This file contains the main configuration settings for the RPC server. 

```json
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
        "enabled": true,
        "replacement_ip": "63.141.233.178"
    }
}
```

### 5. Run the RPC Server
Start the RPC server by running:
```bash
cargo run
```

---

## Understanding Standalone Network Configuration

The `standalone_network` configuration is designed for test or development environments where the consensus nodes and archivers reside on the same server. In such cases, the IP addresses of nodes might use loopback addresses like `127.0.0.1`, which are not accessible from external machines. 

To resolve this, the `replacement_ip` field in the `standalone_network` configuration is used to substitute the loopback IPs with an external-facing IP address. This ensures that the nodes can communicate correctly in a standalone setup.

