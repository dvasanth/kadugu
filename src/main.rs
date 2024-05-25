mod proxyserver;

use std::time::Duration;
use std::net::SocketAddr;
use std::fs::{read, write};
use std::io::Error;
use libp2p::{
    identity::Keypair,
    identify,
    PeerId,
    kad,
    noise, tcp, yamux,
    StreamProtocol,
    multiaddr::Protocol,
    swarm::NetworkBehaviour
};
use futures::stream::StreamExt;
use libp2p_stream as stream;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tokio::net::{TcpListener,TcpStream};
use anyhow::Result;
use async_compat::Compat;
use std::env;


const PROXY_PROTOCOL: StreamProtocol = StreamProtocol::new("/proxy");
const PROXY_AGENT: &str = "libp2p-proxy-vpn";

fn print_usage() {
    println!("Usage:");
    println!("  -s <list of allowed peer ids>   Share internet to all or [optional] allowed peer ids");
    println!("  -u <sharer peer id> Use the shared internet with sharer peer id");
    println!("  -p Print the peer id");
}
enum Mode {
    Sharer,
    User,
    PrintPeerId
}
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    stream: stream::Behaviour
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()?,
        ).init();
    let args: Vec<String> = env::args().collect();

    if args.len() <= 1 {
        print_usage();
        return Ok(());
    }
    let mut accepted_peer_ids = Vec::new();
    let mut mode:Mode= Mode::PrintPeerId;
    let mut sharer_peer_id = PeerId::random();
    if let Some(cmd) = args.get(1).map(|s| s.as_str()) {
        match cmd {
            "-s" => {
                let peer_ids = &args[2..];
                if peer_ids.is_empty() {
                    tracing::info!("Internet shared with anonymous users. Use peer id of known users to prevent unauthorised internet access.");
                } else {
                    tracing::info!("Internet shared only with peer IDs: {:?}", peer_ids);
                }
                accepted_peer_ids = args.iter().skip(2).cloned().collect();
                mode = Mode::Sharer
            }
            "-u" => {
                if let Some(peer_id) = args.get(2) {
                    tracing::info!("Using Internet from peer ID: {}", peer_id);
                   
                    sharer_peer_id = peer_id.parse()?;
                    mode = Mode::User
                } else {
                    tracing::error!("Peer ID not provided for -u option");
                }
            }
            "-p" => {

            }
            _ =>{
                print_usage();
            }
        }
    }
    let key_pair = get_identity().unwrap();
    let peer_id = key_pair.public().to_peer_id();
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(key_pair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()        
        .with_dns()?
        .with_behaviour(|key_pair| Behaviour {
            stream: stream::Behaviour::new(),
            // Create a Kademlia behaviour.
            kademlia: kad::Behaviour::new(
                peer_id,
                kad::store::MemoryStore::new(key_pair.public().to_peer_id()),
            ),
            identify: identify::Behaviour::new(identify::Config::new(
                "/proxy/0.0.1".to_string(),
                key_pair.public(),
            ).with_agent_version(PROXY_AGENT.into())),                 
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    // bootstrap address from ipfs/kubo
    let kubo_peer = "QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ";
    swarm
    .behaviour_mut()
    .kademlia
    .add_address(&kubo_peer.parse()?, "/ip4/104.131.131.82/tcp/4001".parse()?);
    
    if let  Mode::PrintPeerId = mode {
        tracing::info!("This machine PeerId: {:?}", swarm.local_peer_id());
        return Ok(());
    }


    if let  Mode::Sharer = mode  {
        swarm.listen_on("/ip4/0.0.0.0/udp/12007/quic-v1".parse()?)?;
        let incoming_streams = swarm
        .behaviour()
        .stream
        .new_control()
        .accept(PROXY_PROTOCOL)
        .unwrap();

        let _ = swarm
                .behaviour_mut()
                .kademlia
                .bootstrap();
        tokio::spawn(async move {
            // start the proxy server
            let proxy = proxyserver::HttpProxy::new(SocketAddr::from(([127, 0, 0, 1], 8090)));

            if let Err(err) = proxy.run().await {
                tracing::info!("HTTP proxy error: {:?}", err);
            }
        });
        tokio::spawn(async move {
            handle_incoming_streams(incoming_streams, accepted_peer_ids).await;
        });
    } else {
        swarm.listen_on("/ip4/0.0.0.0/udp/12008/quic-v1".parse()?)?;

        swarm
        .behaviour_mut()
        .kademlia
        .get_closest_peers(sharer_peer_id);        
        tracing::info!("Searching for sharer peer id...");
    }
 
    let mut observed_address_seen = false;
    let mut sharer_dial_complete = false;
    // Poll the swarm to make progress.
    loop {
        let event = swarm.next().await.expect("never terminates");

        match event {
            libp2p::swarm::SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    result: kad::QueryResult::GetClosestPeers(Ok(kad::GetClosestPeersOk { peers, .. })),
                    step: kad::ProgressStep {  last , ..},
                    ..
                },
            )) =>{
                if peers.contains(&sharer_peer_id){
                    tracing::info!("Found the sharer peer id");
                } else if last {
                    tracing::info!("Unable to find the Internet sharer peer id. Check if sharer has Internet reachable address.")
                }
            }          
            libp2p::swarm::SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                info: identify::Info { observed_addr, listen_addrs,.. },
                peer_id
            })) => {
                
                let contains_udp = observed_addr.iter().any(|proto| matches!(proto, Protocol::Udp(_))); 
                if  observed_address_seen == false && contains_udp {
                    tracing::info!(address=%observed_addr, "Found Internet reachable address for this peer");
                    swarm.add_external_address(observed_addr);
                    observed_address_seen = true;
                }

                if peer_id == sharer_peer_id && sharer_dial_complete == false {
                    tracing::info!("Found network address of sharer peer {:?}", listen_addrs);
                    swarm.dial(listen_addrs.clone()[0].clone())?;
    
                    tokio::spawn(portforward_connection_handler(
                        peer_id, swarm.behaviour().stream.new_control()));
                    sharer_dial_complete = true;
                }
            }
            event => tracing::trace!(?event),
        }
    }
}

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn portforward_connection_handler(peer: PeerId, mut control: stream::Control) {
    
        tokio::time::sleep(Duration::from_secs(1)).await; // Wait a second between echos.
        //tracing::info!(%peer, "portforward_connection_handler invoked!");

        let addr = SocketAddr::from(([127, 0, 0, 1], 8080)); // Bind to localhost on port 8080
        let listener = TcpListener::bind(addr).await.unwrap();
        tracing::info!("Set your browser proxy setting to 127.0.0.1:8080 to use internet from sharer");
        loop {
            let ( app_stream, _) = listener.accept().await.unwrap();
            let _ = app_stream.set_nodelay(true);
            let  p2p_stream = match control.open_stream(peer, PROXY_PROTOCOL).await {
                Ok(stream) => stream,
                Err(error @ stream::OpenStreamError::UnsupportedProtocol(_)) => {
                    tracing::info!(%peer, %error);
                    return;
                }
                Err(error) => {
                     tracing::info!(%peer, %error);
                    return;
                }
            };
   
            tokio::spawn(async move {
                tracing::info!("Accepted new connection from local");
 
               let mut p2p_tokio_stream =  Compat::new(p2p_stream);
               let mut app_stream = app_stream;

                let (from_p2p, from_app) = match tokio::io::copy_bidirectional(&mut p2p_tokio_stream, &mut app_stream).await {
                    Ok((from_p2p, from_app)) => (from_p2p, from_app),
                    Err(error) => {
                        // Handle the error
                        // For now, let's just print it
                        tracing::info!("Error copying data from app to p2p stream: {:?}", error);
                        return;
                    }
                };
                tracing::info!(
                    "App wrote {} bytes and received {} bytes",
                    from_app, from_p2p
                );
            });
        }
}


async fn handle_incoming_streams(
    mut incoming_streams: stream::IncomingStreams,
    accepted_peer_ids: Vec<String>,
) -> () {

    while let Some((peer, p2p_stream)) = incoming_streams.next().await {
            let peer_id_str = peer.to_string();
            let mut is_accepted = true;
            for accepted_id in &accepted_peer_ids {
                if accepted_id.contains(&peer_id_str) {
                    is_accepted = true;
                    break;
                }
                is_accepted = false;
            }    
            // Check if peer ID is in the allowed vector of strings
            if !is_accepted {
                tracing::warn!("Unauthorized peer: {}", peer_id_str);
                continue;
            }

            tokio::spawn(async move {
                let mut app_stream = TcpStream::connect("127.0.0.1:8090").await.unwrap();
                let _ = app_stream.set_nodelay(true);

                let mut p2p_tokio_stream =  Compat::new(p2p_stream);

                let (from_p2p, from_app) = match tokio::io::copy_bidirectional(&mut p2p_tokio_stream, &mut app_stream).await {
                    Ok((from_p2p, from_app)) => (from_p2p, from_app),
                    Err(error) => {
                        tracing::info!("Error copying data from p2p to app stream: {:?}", error);
                        return;
                    }
                };

                tracing::info!(
                    "P2P stream wrote {} bytes and received {} bytes",
                    from_p2p, from_app
                );
            });
        }
}

// Create new cert key pair if not found otherwise use existing cert.
fn get_identity() -> Result<Keypair, Error> {
    // Define the file path where the key pair will be stored
    let file_path = "identity.keypair";

    // Try to read the key pair from the file
    match read(&file_path) {
        Ok(keypair) => {
            Ok(Keypair::from_protobuf_encoding(keypair.as_slice()).unwrap())
        }
        Err(_) => {
            // If the file doesn't exist or is invalid, generate a new key pair
            let new_keypair = Keypair::generate_ed25519();
            write(&file_path, new_keypair.to_protobuf_encoding().unwrap())?;
            Ok(new_keypair)
        }
    }
}
