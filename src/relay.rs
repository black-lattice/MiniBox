use crate::config::internal::Limits;

#[derive(Debug, Clone, Copy)]
pub struct RelayPlan {
    pub buffer_bytes: usize,
}

impl RelayPlan {
    pub fn from_limits(limits: &Limits) -> Self {
        Self {
            buffer_bytes: limits.relay_buffer_bytes,
        }
    }
}

pub fn relay_plan(limits: &Limits) -> RelayPlan {
    RelayPlan::from_limits(limits)
}
