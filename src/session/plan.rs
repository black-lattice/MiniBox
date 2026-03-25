use crate::config::internal::Limits;
use crate::relay::{RelayPlan, relay_plan};
use crate::upstream::{DirectDialPlan, TrojanDialPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPlan {
    pub relay: RelayPlan,
    pub direct_dial: DirectDialPlan,
    pub trojan_dial: TrojanDialPlan,
}

impl SessionPlan {
    pub fn from_limits(limits: &Limits) -> Self {
        Self {
            relay: relay_plan(limits),
            direct_dial: DirectDialPlan::default(),
            trojan_dial: TrojanDialPlan::default(),
        }
    }
}
