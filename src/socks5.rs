use async_trait::async_trait;
use cidr::{Ipv4Cidr, Ipv6Cidr};
use std::{
    io,
    net::{IpAddr, SocketAddr},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
};

use crate::{
    error::Error,
    metrics::{SOCKS5_ERROR_COUNTER, SOCKS5_REQUEST_COUNTER},
    Server,
};

const SOCKS_VERSION: u8 = 0x05;
const RESERVED: u8 = 0x00;

#[derive(Debug, Clone)]
pub struct Socks5Server {
    addr: std::net::SocketAddr,
    ipv6_subnets: Vec<Ipv6Cidr>,
    ipv4_subnets: Vec<Ipv4Cidr>,
}

#[async_trait]
impl Server for Socks5Server {
    async fn start(&self) -> Result<(), Error> {
        let listener = TcpListener::bind(self.addr).await.map_err(Error::from)?;

        let (ipv4_subnets, ipv6_subnets) = (self.ipv4_subnets.clone(), self.ipv6_subnets.clone());

        tracing::info!("Start SOCKS5 server on {}", self.addr);

        loop {
            let (ipv4_subnets, ipv6_subnets) = (ipv4_subnets.clone(), ipv6_subnets.clone());
            let (mut socket, _addr) = listener.accept().await.map_err(Error::from)?;

            tokio::spawn(async move {
                if let Err(e) = handle_connection(&mut socket, &ipv4_subnets, &ipv6_subnets).await {
                    SOCKS5_ERROR_COUNTER.inc();
                    tracing::error!("SOCKS5 server error: {}", e);
                }
            });
        }
    }
}

impl Socks5Server {
    pub fn new(addr: std::net::SocketAddr) -> Self {
        Self {
            addr,
            ipv4_subnets: vec![],
            ipv6_subnets: vec![],
        }
    }

    pub fn with_ipv4_subnets(mut self, ipv4_subnets: Vec<Ipv4Cidr>) -> Self {
        self.ipv4_subnets = ipv4_subnets;
        self
    }

    pub fn with_ipv6_subnets(mut self, ipv6_subnets: Vec<Ipv6Cidr>) -> Self {
        self.ipv6_subnets = ipv6_subnets;
        self
    }
}

async fn handle_connection(
    socket: &mut TcpStream,
    ipv4_subnets: &[Ipv4Cidr],
    ipv6_subnets: &[Ipv6Cidr],
) -> Result<(), Error> {
    SOCKS5_REQUEST_COUNTER.inc();
    let mut buf = [0; 2];

    socket.read_exact(&mut buf).await.map_err(Error::from)?;

    tracing::debug!("Received SOCKS5 request: {:?}", buf);

    if buf[0] != SOCKS_VERSION {
        return Err(Error::UnsupportedSocksVersion(buf[0]));
    }

    let nmethods = buf[1] as usize;
    let mut methods = vec![0; nmethods];
    socket.read_exact(&mut methods).await.map_err(Error::from)?;

    let selected_method = if methods.contains(&0x00) { 0x00 } else { 0xFF };

    socket.write_all(&[SOCKS_VERSION, selected_method]).await?;

    if selected_method == 0xFF {
        return Err(Error::UnsupportedSocksMethod);
    }

    let mut buf = [0; 4];
    socket.read_exact(&mut buf).await.map_err(Error::from)?;

    let (addr, bind_addr) = match buf[3] {
        0x01 => {
            let mut ipv4 = [0; 4];
            socket.read_exact(&mut ipv4).await.map_err(Error::from)?;
            let port = read_port(socket).await?;
            let addr = SocketAddr::new(IpAddr::V4(ipv4.into()), port);
            let bind_addr = crate::get_rand_ipv4_socket_addr(ipv4_subnets);
            (addr, bind_addr)
        }
        0x03 => {
            let mut domain_len = [0; 1];
            socket
                .read_exact(&mut domain_len)
                .await
                .map_err(Error::from)?;
            let mut domain = vec![0; domain_len[0] as usize];
            socket.read_exact(&mut domain).await.map_err(Error::from)?;
            let port = read_port(socket).await?;
            let domain = String::from_utf8(domain).map_err(|e| Error::FromUtf8Error(e))?;
            let addr_str = format!("{}:{}", domain, port);

            let addr = tokio::net::lookup_host(addr_str.clone())
                .await
                .map_err(Error::from)?
                .next()
                .ok_or(Error::InvalidDomainName(addr_str.clone()))?;

            let bind_addr = match addr {
                SocketAddr::V4(_) => crate::get_rand_ipv4_socket_addr(ipv4_subnets),
                SocketAddr::V6(_) => crate::get_rand_ipv6_socket_addr(ipv6_subnets),
            };
            (addr, bind_addr)
        }
        0x04 => {
            let mut ipv6 = [0; 16];
            socket.read_exact(&mut ipv6).await.map_err(Error::from)?;
            let port = read_port(socket).await?;
            let addr = SocketAddr::new(IpAddr::V6(ipv6.into()), port);
            let bind_addr = crate::get_rand_ipv6_socket_addr(ipv6_subnets);
            (addr, bind_addr)
        }
        addr => return Err(Error::UnsupportedSocksAddressType(addr)),
    };

    let socket_type = match addr {
        SocketAddr::V4(_) => TcpSocket::new_v4(),
        SocketAddr::V6(_) => TcpSocket::new_v6(),
    }
    .map_err(Error::from)?;

    tracing::debug!("Socket bind {}", bind_addr);

    socket_type.bind(bind_addr).map_err(Error::from)?;

    tracing::debug!("Connected to {}", addr);

    let mut remote = socket_type.connect(addr).await.map_err(Error::from)?;

    let reply = SocksReply::new(ResponseCode::Success);
    reply.send(socket).await.map_err(Error::from)?;

    tracing::debug!("Start tunneling");

    tokio::io::copy_bidirectional(socket, &mut remote)
        .await
        .map_err(Error::from)?;

    Ok(())
}

async fn read_port(socket: &mut TcpStream) -> Result<u16, Error> {
    let mut buf = [0; 2];
    socket.read_exact(&mut buf).await.map_err(Error::from)?;
    Ok(u16::from_be_bytes(buf))
}

struct SocksReply {
    buf: [u8; 10],
}

impl SocksReply {
    pub fn new(status: ResponseCode) -> Self {
        let buf = [
            SOCKS_VERSION,
            status as u8,
            RESERVED,
            0x01,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        Self { buf }
    }

    pub async fn send<T>(&self, stream: &mut T) -> io::Result<()>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        stream.write_all(&self.buf).await?;
        Ok(())
    }
}

#[derive(Debug)]
enum ResponseCode {
    Success = 0x00,
}
