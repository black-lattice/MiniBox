mod codec;
mod error;
mod handler;
mod parser;
mod types;

pub use codec::{encode_method_selection, encode_response};
pub use error::{Socks5Error, Socks5HandshakeError};
pub use handler::Socks5Handler;
pub use parser::{parse_greeting, parse_request, parse_target_endpoint};
pub use types::{
    AddressKind, AuthMethod, Command, Greeting, MethodSelection, ReplyCode, Request, Response,
    TargetAddr, TargetEndpoint, VERSION,
};
