<p align="center">
  <img src="https://github.com/dvasanth/kadugu/blob/main/res/kadugu.png" alt="Kadugu Icon">
</p>

# Kadugu VPN - Split connection approach

Kadugu, an innovative open-source project revolutionizing VPN solutions. It harnesses the power of libp2p tunneling to facilitate secure and private HTTP request tunneling, all within a concise codebase of less than 1,000 lines. It offers easy configuration, blazing-fast speeds through QUIC protocol. Setup VPN between machines within seconds. 

## Comparison with other VPNs:
### Architecture 
Kadugu employs a decentralized peer-to-peer architecture, eliminating the need for a central server and offering direct connections between peers.

### Performance
Traditional VPN solutions typically establish a tunnel from the home network to the data center, relying on TCP for end-to-end connectivity. However, TCP can struggle to maintain performance over lossy home networks due to its sensitivity to fluctuating bandwidth.

![image](https://github.com/user-attachments/assets/25772ca9-916e-4487-b9c2-9b620b80042f)

This VPN solution improves upon this bandwidth variation by splitting the connection into two segments: the first, between the home network and the data center, utilizing the QUIC protocol, which is specifically designed to handle lossy and variable networks. The second segment, from the data center to the internet, employs TCP, which excels in the stable and reliable environment of the data center. This hybrid approach results in a VPN connection that offers both low latency and high bandwidth, ensuring a smoother and faster user experience.

### Ease of Use
Typical VPN configuration often involves setting up and managing server configurations, certificates, and client profiles, which can be cumbersome for inexperienced users. Kadugu's single binary deployment and simplified configuration make it easier to deploy and use, requiring minimal setup and maintenance.

## Security
Peers in Kadugu VPN are verified using the Noise protocol over QUIC (a secure transport). During the initial connection, a Noise handshake is performed, where peers exchange cryptographic keys. The Peer IDs are then verified against the exchanged public keys, ensuring that each peer is communicating with the correct identity. All data is encrypted over the libp2p channel, preventing raw traffic from being exposed to the open internet.

## Youtube Video - Configuration with VPN bandwidth testing 

[![Configure](https://img.youtube.com/vi/k2IBeYTIpz4/0.jpg)](https://www.youtube.com/watch?v=k2IBeYTIpz4)


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
