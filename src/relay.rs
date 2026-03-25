use crate::config::internal::Limits;
use tokio::io::{AsyncRead, AsyncWrite, copy_bidirectional_with_sizes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayPlan {
    pub buffer_bytes: usize,
}

impl RelayPlan {
    pub fn from_limits(limits: &Limits) -> Self {
        Self { buffer_bytes: limits.relay_buffer_bytes }
    }
}

pub fn relay_plan(limits: &Limits) -> RelayPlan {
    RelayPlan::from_limits(limits)
}

pub async fn relay_bidirectional<A, B>(
    downstream: &mut A,
    upstream: &mut B,
    plan: RelayPlan,
) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    copy_bidirectional_with_sizes(downstream, upstream, plan.buffer_bytes, plan.buffer_bytes).await
}
