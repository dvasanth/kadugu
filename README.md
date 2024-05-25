<p align="center">
  <img src="https://github.com/dvasanth/kadugu/blob/main/res/kadugu.png" alt="Kadugu Icon">
</p>

# Kadugu

Kadugu, an innovative open-source project revolutionizing VPN solutions. It harnesses the power of libp2p tunneling to facilitate secure and private HTTP request tunneling, all within a concise codebase of less than 1,000 lines. It offers easy configuration, blazing-fast speeds through QUIC protocol. Setup VPN between machines within seconds. 

## Comparison with other VPNs:
Architecture: Kadugu employs a decentralized peer-to-peer architecture, eliminating the need for a central server and offering direct connections between peers.

Performance: Most VPN built reliance on TCP (Transmission Control Protocol) can introduce overhead and latency, especially in high-latency networks. Kadugu's use of QUIC and libp2p tunneling enhances performance by reducing latency and optimizing data transmission.

Ease of Use: Typical VPN configuration often involves setting up and managing server configurations, certificates, and client profiles, which can be cumbersome for inexperienced users. Kadugu's single binary deployment and simplified configuration options make it easier to deploy and use, requiring minimal setup and maintenance.

## Installation

1. Download the latest release from the [releases page](https://github.com/Kadugu/Kadugu/releases).

2. Make the binary executable:
### Linux
```bash
chmod +x kadugu
```

## Usage
### Print Peer Id
1. Peer id uniquely identifies the machine. Person wishing to share internet to others, need to share their ids.
```bash
./kadugu -p
```
### Sharing Internet
1. Run the Kadugu server to share your internet:
```bash
 ./kadugu -s
```
2. Optionally, specify allowed peer IDs to access your internet:
```bash
./kadugu -s <peer_id1>,<peer_id2>,...
```

### Using Shared Internet
1. Run Kadugu client to access shared internet from a peer:
```bash
./kadugu -u <peer_id>
```
Replace <peer_id> with the peer ID of the sharer.
2. Change the browser proxy setting to 127.0.0.1:8080 to use it.

## Contributing
Contributions to Kadugu are welcome! Whether you find a bug, have a feature request, or want to contribute code, please feel free to open an issue or submit a pull request.