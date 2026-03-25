mod connection;
mod direct;
mod resolve;
mod target;
mod trojan;

pub use connection::{UpstreamConnection, UpstreamStream, connect_upstream};
pub use direct::{DialError, DirectDialPlan, dial_tcp};
pub use resolve::{
    ConnectRoute, ConnectRouteKind, ResolveError, resolve_connect_route, resolve_connect_target,
};
pub use target::{DialTarget, DialTargetHost};
pub use trojan::{TrojanDialPlan, TrojanHandshakeStage, TrojanRouteConfig, dial_trojan};
