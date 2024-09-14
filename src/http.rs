use async_trait::async_trait;
use cidr::Ipv6Cidr;
use hyper::{
    client::HttpConnector,
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Body, Client, Method, Request, Response, Server,
};
use rand::Rng;
use std::net::{Ipv6Addr, SocketAddr, ToSocketAddrs};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpSocket,
};

use crate::metrics::{HTTP_ERROR_COUNTER, HTTP_REQUEST_COUNTER};

#[derive(Clone)]
pub struct HttpServer {
    addr: SocketAddr,
    ipv6_subnets: Vec<Ipv6Cidr>,
}

#[async_trait]
impl crate::Server for HttpServer {
    async fn start(&self) -> crate::error::Result<()> {
        let make_service = make_service_fn(move |_: &AddrStream| {
            tracing::info!("Start http server on {}", self.addr);

            let addr = self.addr.clone();
            let ipv6_subnets = self.ipv6_subnets.clone();

            async move {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    HttpServer::new(addr)
                        .with_ipv6_subnets(ipv6_subnets.clone())
                        .proxy(req)
                }))
            }
        });

        Server::bind(&self.addr)
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .serve(make_service)
            .await
            .map_err(|err| err.into())
    }
}

impl HttpServer {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            ipv6_subnets: vec![],
        }
    }

    pub fn with_ipv6_subnets(mut self, ipv6_subnets: Vec<Ipv6Cidr>) -> Self {
        self.ipv6_subnets = ipv6_subnets;
        self
    }

    pub(crate) async fn proxy(self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        HTTP_REQUEST_COUNTER
            .with_label_values(&[req.method().as_str()])
            .inc();
        match if req.method() == Method::CONNECT {
            self.process_connect(req).await
        } else {
            self.process_request(req).await
        } {
            Ok(resp) => Ok(resp),
            Err(e) => {
                tracing::error!("HTTP server error: {}", e);
                HTTP_ERROR_COUNTER.inc();
                Err(e)
            }
        }
    }

    async fn process_connect(self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        tokio::task::spawn(async move {
            let remote_addr = req.uri().authority().map(|auth| auth.to_string()).unwrap();
            let mut upgraded = hyper::upgrade::on(req).await.unwrap();
            self.tunnel(&mut upgraded, remote_addr).await
        });
        Ok(Response::new(Body::empty()))
    }

    async fn process_request(self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        let bind_addr = crate::get_rand_ipv6(self.ipv6_subnets.first().unwrap());
        let mut http = HttpConnector::new();
        http.set_local_address(Some(bind_addr));
        println!("{} via {bind_addr}", req.uri().host().unwrap_or_default());

        let client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build(http);
        let res = client.request(req).await?;
        Ok(res)
    }

    async fn tunnel<A>(self, upgraded: &mut A, addr_str: String) -> std::io::Result<()>
    where
        A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    {
        if let Ok(addrs) = addr_str.to_socket_addrs() {
            for addr in addrs {
                let socket = TcpSocket::new_v6()?;
                let bind_addr = crate::get_rand_ipv6_socket_addr(&self.ipv6_subnets);
                if socket.bind(bind_addr).is_ok() {
                    println!("{addr_str} via {bind_addr}");
                    if let Ok(mut server) = socket.connect(addr).await {
                        tokio::io::copy_bidirectional(upgraded, &mut server).await?;
                        return Ok(());
                    }
                }
            }
        } else {
            println!("error: {addr_str}");
        }

        Ok(())
    }
}
