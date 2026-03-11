use crate::config::internal::Limits;
use crate::relay::{RelayPlan, relay_plan};
use crate::upstream::DirectDialPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPlan {
    pub relay: RelayPlan,
    pub direct_dial: DirectDialPlan,
}

impl SessionPlan {
    pub fn from_limits(limits: &Limits) -> Self {
        Self {
            relay: relay_plan(limits),
            direct_dial: DirectDialPlan::default(),
        }
    }
}
