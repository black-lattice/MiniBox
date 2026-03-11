fn main() {
    let plan = wanglin_proxy::bootstrap::build_startup_plan();

    println!("{}", wanglin_proxy::status_line());
    println!("Current phase: {}", plan.current_phase);
    println!("Clash support boundary: {}", plan.clash_support_boundary);
}
