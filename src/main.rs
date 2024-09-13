use cidr::{Ipv4Cidr, Ipv6Cidr};
use clap::Parser;
use proxy_pool::{http::HttpServer, socks5::Socks5Server, Server};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short = '6', long, env)]
    ipv6_cidr: String,
    #[arg(short = '4', long, env)]
    ipv4_cidr: String,

    #[arg(long, env, default_value = "0.0.0.0")]
    http_host: String,
    #[arg(long, env, default_value = "8080")]
    http_port: u16,

    #[arg(long, env, default_value = "0.0.0.0")]
    socks5_host: String,
    #[arg(long, env, default_value = "8081")]
    socks5_port: u16,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .event_format(
            tracing_subscriber::fmt::format()
                .with_file(true)
                .with_line_number(true),
        )
        .init();
    let cli = Cli::parse();

    let http_addr = format!("{}:{}", cli.http_host, cli.http_port);
    let socks5_addr = format!("{}:{}", cli.socks5_host, cli.socks5_port);

    let ipv6_cidr = parse_subnets::<Ipv6Cidr>(&cli.ipv6_cidr);
    let ipv4_cidr = parse_subnets::<Ipv4Cidr>(&cli.ipv4_cidr);

    let http_server =
        HttpServer::new(http_addr.parse().unwrap()).with_ipv6_subnets(ipv6_cidr.clone());

    let socks5_server = Socks5Server::new(socks5_addr.parse().unwrap())
        .with_ipv6_subnets(ipv6_cidr.clone())
        .with_ipv4_subnets(ipv4_cidr.clone());

    let (http_result, socks5_result) = tokio::join!(http_server.start(), socks5_server.start());

    if let Err(e) = http_result {
        tracing::error!("HTTP server error: {}", e);
    }

    if let Err(e) = socks5_result {
        tracing::error!("SOCKS5 server error: {}", e);
    }
}

fn parse_subnets<C: std::str::FromStr>(subnets: &str) -> Vec<C> {
    subnets
        .split(',')
        .filter_map(|s| s.parse::<C>().ok())
        .collect()
}
