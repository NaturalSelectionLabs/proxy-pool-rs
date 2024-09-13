use std::net::ToSocketAddrs;

use crate::metrics;
use crate::Server;
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};

use cidr::Ipv6Cidr;
use hyper::{body::Incoming, server::conn::http1};
use hyper_util::rt::TokioIo;

use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpSocket},
};
use tower::Service;
use tower::ServiceExt;

#[derive(Clone)]
pub struct HttpServer {
    addr: std::net::SocketAddr,
    ipv6_subnets: Vec<Ipv6Cidr>,
}

#[async_trait]
impl Server for HttpServer {
    async fn start(&self) -> crate::error::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;

        tracing::info!("Start HTTP server on {}", self.addr);

        let router = Router::new().merge(metrics::routes());

        let tower_service = tower::service_fn(move |req: Request<_>| {
            let router = router.clone();
            let req = req.map(Body::new);
            async move {
                if req.method() == Method::CONNECT {
                    proxy(req, self.ipv6_subnets.clone()).await
                } else {
                    router.oneshot(req).await.map_err(|err| match err {})
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
            ipv6_subnets: vec![],
        }
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
