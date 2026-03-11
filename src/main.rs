fn main() {
    let plan = minibox::bootstrap::build_startup_plan();

    println!("{}", minibox::status_line());
    println!("Current phase: {}", plan.current_phase);
    println!("Clash support boundary: {}", plan.clash_support_boundary);
    println!("Operations: {}", plan.operations.summary());
}
