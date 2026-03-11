use crate::session::TargetEndpoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub version: Version,
    pub destination: TargetEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRequest {
    pub destination: TargetEndpoint,
    pub buffered_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCode {
    ConnectionEstablished,
    BadRequest,
    MethodNotAllowed,
    BadGateway,
    RequestHeaderFieldsTooLarge,
}

impl StatusCode {
    pub fn code(self) -> u16 {
        match self {
            Self::ConnectionEstablished => 200,
            Self::BadRequest => 400,
            Self::MethodNotAllowed => 405,
            Self::BadGateway => 502,
            Self::RequestHeaderFieldsTooLarge => 431,
        }
    }

    pub fn reason_phrase(self) -> &'static str {
        match self {
            Self::ConnectionEstablished => "Connection Established",
            Self::BadRequest => "Bad Request",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::BadGateway => "Bad Gateway",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
        }
    }
}
