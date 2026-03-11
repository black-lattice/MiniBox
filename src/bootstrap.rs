use crate::subscription::SubscriptionPlan;

#[derive(Debug, Clone)]
pub struct StartupPlan {
    pub current_phase: &'static str,
    pub clash_support_boundary: &'static str,
    pub steps: &'static [&'static str],
    pub subscription: SubscriptionPlan,
}

pub fn build_startup_plan() -> StartupPlan {
    StartupPlan {
        current_phase: "listener accept-loop + direct CONNECT proxy baseline",
        clash_support_boundary: "level B: nodes + groups, without full rule compatibility",
        steps: &[
            "validate internal config snapshot",
            "prepare listener registry and shared admission control",
            "bind configured listeners and accept downstream TCP sessions",
            "parse SOCKS5 and HTTP CONNECT requests into session targets",
            "add relay pipeline",
            "add metrics and logging",
            "add Clash adapter and cache rollback",
        ],
        subscription: SubscriptionPlan::default(),
    }
}
