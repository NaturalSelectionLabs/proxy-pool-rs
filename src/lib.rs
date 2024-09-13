pub mod error;
pub mod http;
pub mod metrics;
pub mod socks5;

use async_trait::async_trait;
use cidr::{Ipv4Cidr, Ipv6Cidr};
use std::net::{IpAddr, SocketAddr};

use error::Error;
use rand::random;
use rand::seq::SliceRandom;

#[async_trait]
pub trait Server {
    async fn start(&self) -> Result<(), Error>;
}

pub fn get_rand_ipv4_socket_addr(ipv4_subnets: &[Ipv4Cidr]) -> SocketAddr {
    let mut rng = rand::thread_rng();
    let ipv4_cidr = ipv4_subnets.choose(&mut rng).unwrap();
    let ip_addr = get_rand_ipv4(ipv4_cidr);
    SocketAddr::new(ip_addr, random::<u16>())
}

pub fn get_rand_ipv6_socket_addr(ipv6_subnets: &[Ipv6Cidr]) -> SocketAddr {
    let mut rng = rand::thread_rng();
    let ipv6_cidr = ipv6_subnets.choose(&mut rng).unwrap();
    let ip_addr = get_rand_ipv6(ipv6_cidr);
    SocketAddr::new(ip_addr, random::<u16>())
}

pub fn get_rand_ipv4(ipv4_cidr: &Ipv4Cidr) -> IpAddr {
    let mut ipv4 = u32::from(ipv4_cidr.first_address());
    if ipv4_cidr.network_length() != 32 {
        let rand: u32 = random();
        let net_part =
            (ipv4 >> (32 - ipv4_cidr.network_length())) << (32 - ipv4_cidr.network_length());
        let host_part = (rand << ipv4_cidr.network_length()) >> ipv4_cidr.network_length();
        ipv4 = net_part | host_part;
    }
    IpAddr::V4(ipv4.into())
}

pub fn get_rand_ipv6(ipv6_cidr: &Ipv6Cidr) -> IpAddr {
    let mut ipv6 = u128::from(ipv6_cidr.first_address());
    if ipv6_cidr.network_length() != 128 {
        let rand: u128 = random();
        let net_part =
            (ipv6 >> (128 - ipv6_cidr.network_length())) << (128 - ipv6_cidr.network_length());
        let host_part = (rand << ipv6_cidr.network_length()) >> ipv6_cidr.network_length();
        ipv6 = net_part | host_part;
    }
    IpAddr::V6(ipv6.into())
}
