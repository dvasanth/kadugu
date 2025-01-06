mod proxyserver;

use libp2p::Multiaddr;
use libp2p::{
    dcutr, identify, identity::Keypair, multiaddr::Protocol, noise, relay, swarm::NetworkBehaviour,
    tcp, yamux, PeerId, StreamProtocol,
};
use std::cmp::PartialEq;
use std::fs::{read, write};
use std::io::Error;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use async_compat::Compat;
use clap::{crate_description, crate_version, Arg, ArgAction, Command};
use futures::stream::StreamExt;
use libp2p_stream as stream;
use tokio::net::{TcpListener, TcpStream};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const PROXY_PROTOCOL: StreamProtocol = StreamProtocol::new("/proxy");
const PROXY_AGENT: &str = "libp2p-proxy-vpn";

#[derive(PartialEq)]
enum Mode {
    Sharer,
    User,
    PrintPeerId,
    Undefined,
}
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    stream: stream::Behaviour,
    relay_client: relay::client::Behaviour,
    dcutr: dcutr::Behaviour,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .parse("kadugu")?,
        )
        .init();

    let matches = Command::new("kadugu")
        .version(crate_version!())
        .about(crate_description!())
        .arg_required_else_help(true)
        .arg(
            Arg::new("sharer")
                .short('s')
                .long("sharer")
                .value_name("ids")
                .num_args(0..)
                .value_delimiter(',')
                .help("Share internet to all or specific peer IDs"),
        )
        .arg(
            Arg::new("user")
                .short('u')
                .long("user")
                .value_name("id")
                .help("Peer ID of the sharer to connect to"),
        )
        .arg(
            Arg::new("print-peer-id")
                .short('p')
                .long("print-peer-id")
                .action(ArgAction::SetTrue)
                .help("Print the local Peer ID"),
        )
        .arg(
            Arg::new("expose-lan")
                .short('e')
                .long("expose-lan")
                .action(ArgAction::SetTrue)
                .help("Expose the internet to other machines in the local network. Use it with -u option"),
        )
        .get_matches();

    let mut accepted_peer_ids = Vec::new();
    let mut mode: Mode = Mode::Undefined;
    let mut sharer_peer_id = PeerId::random();
    let mut proxy_listen_addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 8080));

    if let Some(peer_ids) = matches.get_many::<String>("sharer") {
        accepted_peer_ids = peer_ids.map(|id| id.to_string()).collect();
        if accepted_peer_ids.is_empty() {
            tracing::info!("Internet shared with anonymous users. Use peer id of known users to prevent unauthorised internet access.");
        } else {
            tracing::info!(
                "Internet shared only with peer IDs: {:?}",
                accepted_peer_ids
            );
        }
        mode = Mode::Sharer;
    } else if let Some(peer_id) = matches.get_one::<String>("user") {
        sharer_peer_id = peer_id.parse()?;
        tracing::info!("Using Internet from peer ID: {}", sharer_peer_id);
        mode = Mode::User;

        if matches.get_flag("expose-lan") {
            proxy_listen_addr = SocketAddr::from(([0, 0, 0, 0], 8080));
        }
    }

    if matches.get_flag("print-peer-id") {
        mode = Mode::PrintPeerId;
    }

    if mode == Mode::Undefined {
        tracing::error!("Please specify either --sharer or --user or --print-peer-id");
        return Ok(());
    }

    let key_pair = get_identity().unwrap();
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(key_pair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_relay_client(noise::Config::new, yamux::Config::default)?
        .with_behaviour(|key_pair, relay_behaviour| Behaviour {
            stream: stream::Behaviour::new(),
            identify: identify::Behaviour::new(
                identify::Config::new("/proxy/0.0.1".to_string(), key_pair.public())
                    .with_agent_version(PROXY_AGENT.into()),
            ),
            relay_client: relay_behaviour,
            dcutr: dcutr::Behaviour::new(key_pair.public().to_peer_id()),
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    let relay_address: Multiaddr =
        "/ip4/104.131.131.82/udp/4001/quic-v1/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ"
            .parse()?;

    if let Mode::PrintPeerId = mode {
        tracing::info!("This machine PeerId: {:?}", swarm.local_peer_id());
        return Ok(());
    }
    swarm.listen_on("/ip4/0.0.0.0/udp/12007/quic-v1".parse()?)?;
    swarm.listen_on("/ip6/::/udp/12007/quic-v1".parse()?)?;
    swarm.dial(relay_address.clone()).unwrap();

    if let Mode::Sharer = mode {
        let incoming_streams = swarm
            .behaviour()
            .stream
            .new_control()
            .accept(PROXY_PROTOCOL)
            .unwrap();

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
        tracing::info!("Searching for sharer peer id...");
    }

    let mut sharer_dial_complete = false;
    let mut relay_reservation_complete = false;
    // Poll the swarm to make progress.
    loop {
        let event = swarm.next().await.expect("never terminates");

        match event {
            libp2p::swarm::SwarmEvent::ExternalAddrExpired { .. } => {
                relay_reservation_complete = false;
            }
            libp2p::swarm::SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
            )) => {
                tracing::info!("Reservation with relay {:?} completed ", relay_peer_id);
                relay_reservation_complete = true;
            }
            libp2p::swarm::SwarmEvent::OutgoingConnectionError {
                connection_id: _,
                peer_id,
                ..
            } => {
                if peer_id.is_some_and(|id| id == sharer_peer_id) {
                    //sharer_dial_complete = false;
                    swarm
                        .dial(
                            relay_address
                                .clone()
                                .with(Protocol::P2pCircuit)
                                .with(Protocol::P2p(sharer_peer_id)),
                        )
                        .unwrap();
                }
            }
            libp2p::swarm::SwarmEvent::Behaviour(BehaviourEvent::Identify(
                identify::Event::Received { .. },
            )) => {
                if let Mode::Sharer = mode {
                    if relay_reservation_complete == false {
                        swarm
                            .listen_on(relay_address.clone().with(Protocol::P2pCircuit))
                            .unwrap();
                    }
                } else {
                    if sharer_dial_complete == false {
                        swarm
                            .dial(
                                relay_address
                                    .clone()
                                    .with(Protocol::P2pCircuit)
                                    .with(Protocol::P2p(sharer_peer_id)),
                            )
                            .unwrap();
                        tokio::spawn(portforward_connection_handler(
                            sharer_peer_id,
                            swarm.behaviour().stream.new_control(),
                            proxy_listen_addr,
                        ));
                        sharer_dial_complete = true;
                    }
                }
            }
            event => tracing::trace!(?event),
            //_ => {}
        }
    }
}

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn portforward_connection_handler(
    peer: PeerId,
    mut control: stream::Control,
    proxy_listen_addr: SocketAddr,
) {
    let listener = TcpListener::bind(proxy_listen_addr).await.unwrap();
    tracing::info!("Set your browser proxy setting to 127.0.0.1:8080 to use internet from sharer");
    loop {
        let (app_stream, _) = listener.accept().await.unwrap();
        let _ = app_stream.set_nodelay(true);

        let p2p_stream = match control.open_stream(peer, PROXY_PROTOCOL).await {
            Ok(stream) => stream,
            Err(error @ stream::OpenStreamError::UnsupportedProtocol(_)) => {
                tracing::info!(%peer, %error);
                continue;
            }
            Err(error) => {
                tracing::info!(%peer, %error);
                continue;
            }
        };

        tokio::spawn(async move {
            tracing::info!("Accepted new connection from local");

            let mut p2p_tokio_stream = Compat::new(p2p_stream);
            let mut app_stream = app_stream;

            let (from_p2p, from_app) =
                match tokio::io::copy_bidirectional(&mut p2p_tokio_stream, &mut app_stream).await {
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
                from_app,
                from_p2p
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

            let mut p2p_tokio_stream = Compat::new(p2p_stream);

            let (from_p2p, from_app) =
                match tokio::io::copy_bidirectional(&mut p2p_tokio_stream, &mut app_stream).await {
                    Ok((from_p2p, from_app)) => (from_p2p, from_app),
                    Err(error) => {
                        tracing::info!("Error copying data from p2p to app stream: {:?}", error);
                        return;
                    }
                };

            tracing::info!(
                "P2P stream wrote {} bytes and received {} bytes",
                from_p2p,
                from_app
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
        Ok(keypair) => Ok(Keypair::from_protobuf_encoding(keypair.as_slice()).unwrap()),
        Err(_) => {
            // If the file doesn't exist or is invalid, generate a new key pair
            let new_keypair = Keypair::generate_ed25519();
            write(&file_path, new_keypair.to_protobuf_encoding().unwrap())?;
            Ok(new_keypair)
        }
    }
}
