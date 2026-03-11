use crate::protocol::socks5::types::{
    MethodSelection, Response, TargetAddr, TargetEndpoint, VERSION,
};

pub fn encode_method_selection(selection: MethodSelection) -> [u8; 2] {
    [VERSION, selection.method.to_byte()]
}

pub fn encode_response(response: &Response) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(encoded_endpoint_len(&response.bind) + 3);
    encoded.push(VERSION);
    encoded.push(response.reply.to_byte());
    encoded.push(0x00);
    encode_endpoint(&response.bind, &mut encoded);
    encoded
}

fn encoded_endpoint_len(endpoint: &TargetEndpoint) -> usize {
    match &endpoint.address {
        TargetAddr::Ipv4(_) => 1 + 4 + 2,
        TargetAddr::Domain(domain) => 1 + 1 + domain.len() + 2,
        TargetAddr::Ipv6(_) => 1 + 16 + 2,
    }
}

fn encode_endpoint(endpoint: &TargetEndpoint, output: &mut Vec<u8>) {
    match &endpoint.address {
        TargetAddr::Ipv4(address) => {
            output.push(endpoint.address.kind().to_byte());
            output.extend_from_slice(&address.octets());
        }
        TargetAddr::Domain(domain) => {
            output.push(endpoint.address.kind().to_byte());
            output.push(domain.len() as u8);
            output.extend_from_slice(domain.as_bytes());
        }
        TargetAddr::Ipv6(address) => {
            output.push(endpoint.address.kind().to_byte());
            output.extend_from_slice(&address.octets());
        }
    }

    output.extend_from_slice(&endpoint.port.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{encode_method_selection, encode_response};
    use crate::protocol::socks5::{
        AuthMethod, MethodSelection, ReplyCode, Response, TargetAddr, TargetEndpoint,
    };

    #[test]
    fn encodes_no_auth_selection() {
        assert_eq!(
            encode_method_selection(MethodSelection {
                method: AuthMethod::NoAuth,
            }),
            [0x05, 0x00]
        );
    }

    #[test]
    fn encodes_success_response() {
        let response = Response {
            reply: ReplyCode::Succeeded,
            bind: TargetEndpoint {
                address: TargetAddr::Ipv4(Ipv4Addr::new(127, 0, 0, 1)),
                port: 1080,
            },
        };

        assert_eq!(
            encode_response(&response),
            vec![0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x04, 0x38]
        );
    }
}
