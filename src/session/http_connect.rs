use crate::session::{SessionContext, SessionError};

pub fn placeholder_error(context: &SessionContext) -> SessionError {
    SessionError::unimplemented(format!(
        "HTTP CONNECT downstream handling for listener '{}' remains a placeholder",
        context.listener_name
    ))
}
