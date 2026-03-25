use std::process::ExitCode;

use minibox::bootstrap::{StartupOptions, build_startup_plan, run, startup_source_from_arg};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let source = match (args.next(), args.next()) {
        (Some(source), None) => startup_source_from_arg(&source),
        (None, None) => build_startup_plan().subscription.source,
        _ => {
            eprintln!("usage: minibox [local-config-path|http(s)://clash-subscription]");
            eprintln!("example: minibox");
            eprintln!("example: minibox http://example.com/subscription");
            return ExitCode::from(2);
        }
    };

    match run(StartupOptions::from_source(source)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}
