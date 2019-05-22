#[macro_use]
extern crate clap;
extern crate bytes;
extern crate futures;
extern crate http;
extern crate hyper;
extern crate prost;
extern crate tokio;
extern crate tower_grpc;
extern crate tower_hyper;
extern crate tower_request_modifier;
extern crate tower_service;
extern crate tower_util;

use clap::{App, Arg, SubCommand};
use futures::Future;
use hyper::client::connect::{Destination, HttpConnector};
use std::net::ToSocketAddrs;
use tower_grpc::Request;
use tower_hyper::{client, util};
use tower_util::MakeService;

pub mod node {
    include!(concat!(env!("OUT_DIR"), "/ensicoin_rpc.rs"));
}

use node::{Address, ConnectPeerRequest, DisconnectPeerRequest, GetInfoRequest, Peer};

fn is_address(s: String) -> Result<(), String> {
    let mut addrs = match s.to_socket_addrs() {
        Ok(a) => a,
        Err(e) => return Err(format!("cannot parse address: {}", e)),
    };
    match addrs.next() {
        Some(_) => Ok(()),
        None => Err("Could not resolve address".to_string()),
    }
}

fn is_uri(s: String) -> Result<(), String> {
    match s.parse::<http::Uri>() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Invalid URI: {}", e)),
    }
}

fn find_ipv4(s: &str) -> Option<std::net::SocketAddr> {
    s.to_socket_addrs().unwrap().find(|s| s.is_ipv4())
}

fn build_cli() -> App<'static, 'static> {
    app_from_crate!()
        .arg(
            Arg::with_name("uri")
                .short("a")
                .long("address")
                .help("gRPC address of the node")
                .required(true)
                .takes_value(true)
                .validator(is_uri)
                .default_value("http://localhost:4225")
                .conflicts_with("completions"),
        )
        .arg(
            Arg::with_name("completions")
                .long("completions")
                .help("Generates completion scripts for your shell")
                .possible_values(&["bash", "fish", "zsh"])
                .takes_value(true),
        )
        .subcommand(
            SubCommand::with_name("connect")
                .about("Connect the node to a remote peer")
                .arg(
                    Arg::with_name("address")
                        .takes_value(true)
                        .required(true)
                        .validator(is_address),
                ),
        )
        .subcommand(
            SubCommand::with_name("disconnect")
                .about("Disconnect the node from a peer")
                .arg(
                    Arg::with_name("address")
                        .takes_value(true)
                        .required(true)
                        .validator(is_address),
                ),
        )
        .subcommand(SubCommand::with_name("getinfo").about("Gets some information on the node"))
}

fn main() {
    let matches = build_cli().get_matches();

    if matches.is_present("completions") {
        let shell = matches.value_of("completions").unwrap();
        build_cli().gen_completions_to("arc-cli", shell.parse().unwrap(), &mut std::io::stdout());
        return;
    }

    let uri: http::Uri = matches.value_of("uri").unwrap().parse().unwrap();
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

    match matches.subcommand() {
        ("connect", Some(submatches)) => {
            let addrs = submatches.value_of("address").unwrap();
            let socket_addr = match find_ipv4(addrs) {
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
        ("disconnect", Some(submatches)) => {
            let addrs = submatches.value_of("address").unwrap();
            let socket_addr = match find_ipv4(addrs) {
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
        ("getinfo", _) => {
            let info_req = rg.and_then(|mut client| {
                client
                    .get_info(Request::new(GetInfoRequest {}))
                    .map_err(|e| eprintln!("Error retrieving information: {}", e))
                    .and_then(|response| {
                        let response = response.into_inner();
                        println!("Informations: ");
                        println!("\tNode:");
                        println!("\t\tImplementation: {}", &response.implementation);
                        println!("\t\tVersion: {}", response.protocol_version);
                        println!("\tBlockchain: ");
                        println!(
                            "\t\tBest Block Hash: {}",
                            response
                                .best_block_hash
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .fold(String::new(), |mut acc, hb| {
                                    acc.push_str(&hb);
                                    acc
                                })
                        );
                        println!(
                            "\t\tGenesis Hash: {}",
                            response
                                .genesis_block_hash
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .fold(String::new(), |mut acc, hb| {
                                    acc.push_str(&hb);
                                    acc
                                })
                        );
                        Ok(())
                    })
            });
            tokio::run(info_req);
        }
        _ => (),
    }
}
