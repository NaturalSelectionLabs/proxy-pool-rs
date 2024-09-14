use thiserror::Error;
#[derive(Error, Debug)]
pub enum Error {
    #[error("std error: {0}")]
    StdError(#[source] std::io::Error),

    #[error("axum http error: {0}")]
    AxumHttpError(#[source] axum::http::Error),

    #[error("From UTF8 error {0}")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),

    #[error("From hyper error {0}")]
    FromHyperError(#[from] hyper::Error),

    #[error("Unsupported SOCKS version, {0}")]
    UnsupportedSocksVersion(u8),

    #[error("Unsupported SOCKS method")]
    UnsupportedSocksMethod,

    #[error("Unsupported SOCKS address type, {0}")]
    UnsupportedSocksAddressType(u8),

    #[error("Invalid domain name {0}")]
    InvalidDomainName(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::StdError(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
