use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};

use crate::error::Error;
use crate::listener::{ListenerPlan, ListenerRegistry, PreparedListener};

#[derive(Debug)]
pub struct BoundListener {
    plan: ListenerPlan,
    local_addr: SocketAddr,
    listener: TcpListener,
}

impl BoundListener {
    pub fn plan(&self) -> &ListenerPlan {
        &self.plan
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr), std::io::Error> {
        self.listener.accept().await
    }
}

pub async fn bind_listener(plan: ListenerPlan) -> Result<BoundListener, Error> {
    let listener = TcpListener::bind(&plan.bind).await.map_err(|error| {
        Error::io(format!(
            "failed to bind listener '{}' ({:?}) on '{}': {error}",
            plan.name, plan.protocol, plan.bind
        ))
    })?;
    let local_addr = listener.local_addr().map_err(|error| {
        Error::io(format!(
            "listener '{}' ({:?}) bound on '{}' but local address lookup failed: {error}",
            plan.name, plan.protocol, plan.bind
        ))
    })?;

    Ok(BoundListener {
        plan,
        local_addr,
        listener,
    })
}

pub async fn bind_prepared_listener(listener: PreparedListener) -> Result<BoundListener, Error> {
    bind_listener(listener.plan().clone()).await
}

pub async fn bind_registry(registry: &ListenerRegistry) -> Result<Vec<BoundListener>, Error> {
    let mut bound = Vec::with_capacity(registry.listeners().len());

    for listener in registry.listeners() {
        bound.push(bind_prepared_listener(listener.clone()).await?);
    }

    Ok(bound)
}
