use crate::protocol::http_connect::StatusCode;

pub fn encode_response(status: StatusCode) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {} {}\r\n", status.code(), status.reason_phrase());

    match status {
        StatusCode::ConnectionEstablished => {
            response.push_str("\r\n");
        }
        StatusCode::MethodNotAllowed => {
            response.push_str("Allow: CONNECT\r\n");
            response.push_str("Content-Length: 0\r\n");
            response.push_str("Connection: close\r\n\r\n");
        }
        StatusCode::BadRequest
        | StatusCode::BadGateway
        | StatusCode::RequestHeaderFieldsTooLarge => {
            response.push_str("Content-Length: 0\r\n");
            response.push_str("Connection: close\r\n\r\n");
        }
    }

    response.into_bytes()
}
