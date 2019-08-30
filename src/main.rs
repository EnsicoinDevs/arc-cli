use futures::Future;
use hyper::client::connect::{Destination, HttpConnector};
use std::net::ToSocketAddrs;
use structopt::StructOpt;
use tower_grpc::Request;
use tower_hyper::{client, util};
use tower_util::MakeService;

pub mod node {
    include!(concat!(env!("OUT_DIR"), "/ensicoin_rpc.rs"));
}

use node::{Address, ConnectPeerRequest, DisconnectPeerRequest, GetInfoRequest, Peer};

#[derive(StructOpt)]
#[structopt(name = "arc-cli", about = "A CLI to use with an ensicoin node")]
struct Config {
    #[structopt(
        about = "The address of the local node",
        default_value = "http://localhost:4225"
    )]
    node_address: http::Uri,
    #[structopt(subcommand)]
    action: Action,
}

#[derive(StructOpt)]
enum Action {
    #[structopt(about = "information on the node")]
    GetInfo,
    #[structopt(about = "connect to another node")]
    Connect { address: String },
    #[structopt(about = "disconnect from another node")]
    Disconnect { address: String },
}

fn find_ipv4(s: &str) -> Option<std::net::SocketAddr> {
    s.to_socket_addrs().unwrap().find(|s| s.is_ipv4())
}

fn print_getinfo(
    implementation: &str,
    protocol_version: u32,
    best_block_hash: &str,
    genesis_hash: &str,
) {
    use yansi::Paint;
    println!("{}", Paint::green("Node information").underline().bold());
    println!("    {}", Paint::new("Node").underline().bold());
    println!(
        "        {}: {}",
        Paint::new("Name").underline(),
        implementation
    );
    println!(
        "        {}: {}",
        Paint::new("Protocol version").underline(),
        protocol_version
    );
    println!("    {}", Paint::new("Chain").underline().bold());
    println!(
        "        {}: {}",
        Paint::new("Best hash").underline(),
        best_block_hash
    );
    println!(
        "        {}: {}",
        Paint::new("Genesis hash").underline(),
        genesis_hash
    );
}

fn main() {
    let config = Config::from_args();

    let uri: http::Uri = config.node_address;
    let dst = match Destination::try_from_uri(uri.clone()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Could not connect to {}: {}", uri, e);
            return;
        }
    };
    let connector = util::Connector::new(HttpConnector::new(4));
    let settings = client::Builder::new().http2_only(true).clone();
    let mut make_client = client::Connect::with_builder(connector, settings);
    let rg = make_client
        .make_service(dst)
        .map_err(|e| {
            eprintln!("HTTP/2 connection failed: {}", e);
        })
        .and_then(move |conn| {
            use node::client::Node;
            let conn = tower_request_modifier::Builder::new()
                .set_origin(uri)
                .build(conn)
                .unwrap();

            Node::new(conn)
                .ready()
                .map_err(|e| eprintln!("client closed: {}", e))
        });

    match config.action {
        Action::GetInfo => {
            let info_req = rg.and_then(|mut client| {
                client
                    .get_info(Request::new(GetInfoRequest {}))
                    .map_err(|e| eprintln!("Error retrieving information: {}", e))
                    .and_then(|response| {
                        let response = response.into_inner();
                        print_getinfo(
                            &response.implementation,
                            response.protocol_version,
                            &response
                                .best_block_hash
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .fold(String::new(), |mut acc, hb| {
                                    acc.push_str(&hb);
                                    acc
                                }),
                            &response
                                .genesis_block_hash
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .fold(String::new(), |mut acc, hb| {
                                    acc.push_str(&hb);
                                    acc
                                }),
                        );
                        Ok(())
                    })
            });
            tokio::run(info_req);
        }
        Action::Connect { address } => {
            let socket_addr = match find_ipv4(&address) {
                Some(a) => a,
                None => {
                    eprintln!("Could not resolve to ipv4");
                    return;
                }
            };
            let conn_req = rg.and_then(move |mut client| {
                let address = Address {
                    ip: format!("{}", socket_addr.ip()),
                    port: socket_addr.port() as u32,
                };
                let peer = Peer {
                    address: Some(address),
                };
                client
                    .connect_peer(Request::new(ConnectPeerRequest { peer: Some(peer) }))
                    .map_err(|e| eprintln!("Could not connect to peer: {}", e))
                    .map(|_| ())
            });
            tokio::run(conn_req)
        }
        Action::Disconnect { address } => {
            let socket_addr = match find_ipv4(&address) {
                Some(a) => a,
                None => {
                    eprintln!("Could not resolve to ipv4");
                    return;
                }
            };
            let conn_req = rg.and_then(move |mut client| {
                let address = Address {
                    ip: format!("{}", socket_addr.ip()),
                    port: socket_addr.port() as u32,
                };
                let peer = Peer {
                    address: Some(address),
                };
                client
                    .disconnect_peer(Request::new(DisconnectPeerRequest { peer: Some(peer) }))
                    .map_err(|e| eprintln!("Could not connect to peer: {}", e))
                    .map(|_| ())
            });
            tokio::run(conn_req)
        }
    }
}
