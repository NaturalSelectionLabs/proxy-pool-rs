use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

use crate::Server;
use crate::{get_rand_ipv4_socket_addr, get_rand_ipv6_socket_addr};
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use cidr::Ipv4Cidr;
use cidr::Ipv6Cidr;
use hyper::{body::Incoming, server::conn::http1};
use hyper_util::rt::TokioIo;
use rand::seq::SliceRandom;

use hyper_util::{client::legacy::connect::HttpConnector, rt::TokioExecutor};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpSocket},
};
use tower::Service;

type Client = hyper_util::client::legacy::Client<HttpConnector, Body>;

#[derive(Clone)]
pub struct HttpServer {
    addr: std::net::SocketAddr,
    ipv4_subnets: Vec<Ipv4Cidr>,
    ipv6_subnets: Vec<Ipv6Cidr>,
}

#[async_trait]
impl Server for HttpServer {
    async fn start(&self) -> crate::error::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;

        tracing::info!("Start HTTP server on {}", self.addr);

        let tower_service = tower::service_fn(move |req: Request<_>| {
            let req = req.map(Body::new);
            async move {
                tracing::debug!("Request method: {:?}", req.method());
                if req.method() == Method::CONNECT {
                    proxy(req, self.ipv6_subnets.clone()).await
                } else {
                    request(req, &self.ipv4_subnets.clone(), &self.ipv6_subnets.clone()).await
                    // router.oneshot(req).await.map_err(|err| match err {})
                }
            }
        });
        let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
            tower_service.clone().call(request)
        });

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let hyper_service = hyper_service.clone();
            if let Err(err) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(io, hyper_service)
                .with_upgrades()
                .await
            {
                tracing::warn!("Failed to serve connection: {:?}", err);
            }
        }
    }
}

impl HttpServer {
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

async fn proxy(req: Request, ipv6_subnets: Vec<Ipv6Cidr>) -> Result<Response, hyper::Error> {
    tracing::trace!(?req);

    if let Some(host_addr) = req.uri().authority().map(|auth| auth.to_string()) {
        tokio::task::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    let mut upgraded = TokioIo::new(upgraded);
                    if let Err(e) = tunnel(&mut upgraded, host_addr, ipv6_subnets).await {
                        tracing::warn!("server io error: {}", e);
                    };
                }
                Err(e) => tracing::warn!("upgrade error: {}", e),
            }
        });

        Ok(Response::new(Body::empty()))
    } else {
        tracing::warn!("CONNECT host is not socket addr: {:?}", req.uri());
        Ok((
            StatusCode::BAD_REQUEST,
            "CONNECT must be to a socket address",
        )
            .into_response())
    }
}

async fn request(
    req: Request,
    ipv4_subnets: &[Ipv4Cidr],
    ipv6_subnets: &[Ipv6Cidr],
) -> Result<Response, hyper::Error> {
    tracing::trace!(?req);

    let bind_addr = if let Some(host) = req.uri().host() {
        let addr_str = format!("{}:{}", host, req.uri().port_u16().unwrap_or(80));

        match tokio::net::lookup_host(addr_str).await {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    match addr {
                        SocketAddr::V4(_) => {
                            // Host resolves to an IPv4 address, select from IPv4 subnets
                            if let Some(ipv4_cidr) = ipv4_subnets.choose(&mut rand::thread_rng()) {
                                get_rand_ipv4_socket_addr(std::slice::from_ref(ipv4_cidr)).ip()
                            } else {
                                IpAddr::V4(Ipv4Addr::LOCALHOST) // Fallback to IPv4 loopback address (127.0.0.1)
                            }
                        }
                        SocketAddr::V6(_) => {
                            // Host resolves to an IPv6 address, select from IPv6 subnets
                            if let Some(ipv6_cidr) = ipv6_subnets.choose(&mut rand::thread_rng()) {
                                get_rand_ipv6_socket_addr(std::slice::from_ref(ipv6_cidr)).ip()
                            } else {
                                IpAddr::V6(Ipv6Addr::LOCALHOST) // Fallback to IPv6 loopback address (::1)
                            }
                        }
                    }
                } else {
                    // No valid address found, fallback to loopback
                    if ipv6_subnets.is_empty() {
                        IpAddr::V4(Ipv4Addr::LOCALHOST) // Default to IPv4 loopback
                    } else {
                        IpAddr::V6(Ipv6Addr::LOCALHOST) // Default to IPv6 loopback
                    }
                }
            }
            Err(_) => {
                // Error during lookup, fallback to loopback
                if ipv6_subnets.is_empty() {
                    IpAddr::V4(Ipv4Addr::LOCALHOST) // Default to IPv4 loopback
                } else {
                    IpAddr::V6(Ipv6Addr::LOCALHOST) // Default to IPv6 loopback
                }
            }
        }
    } else {
        // Fallback if there is no host in the URI
        if ipv6_subnets.is_empty() {
            IpAddr::V4(Ipv4Addr::LOCALHOST) // Default to IPv4 loopback
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST) // Default to IPv6 loopback
        }
    };

    let mut http = HttpConnector::new();
    http.set_local_address(Some(bind_addr));
    tracing::info!("{} via {}", req.uri().host().unwrap_or_default(), bind_addr);

    // Apply timeout to the HTTP request process
    let req = async {
        // let client = Client::builder()
        //     .http1_title_case_headers(true)
        //     .http1_preserve_header_case(true)
        //     .build(http);

        let client: Client =
            hyper_util::client::legacy::Client::<(), ()>::builder(TokioExecutor::new())
                .http1_title_case_headers(true)
                .http1_preserve_header_case(true)
                .build(http);

        client.request(req).await
    };

    Ok(req
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)
        .into_response())
}

async fn tunnel<T>(
    upgraded: &mut T,
    addr_str: String,
    ipv6_subnets: Vec<Ipv6Cidr>,
) -> std::io::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    if let Ok(addrs) = addr_str.to_socket_addrs() {
        for addr in addrs {
            let socket = TcpSocket::new_v6()?;
            let bind_addr = crate::get_rand_ipv6_socket_addr(&ipv6_subnets.clone());
            if socket.bind(bind_addr).is_ok() {
                tracing::info!("{addr_str} via {bind_addr}");
                if let Ok(mut server) = socket.connect(addr).await {
                    let (from_client, from_server) =
                        tokio::io::copy_bidirectional(upgraded, &mut server).await?;
                    tracing::debug!(
                        "client wrote {} bytes and received {} bytes",
                        from_client,
                        from_server
                    );
                    return Ok(());
                }
            }
        }
    } else {
        tracing::error!("error: {addr_str}")
    }

    Ok(())
}
