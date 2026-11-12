//! Binary entry point for `rusty-compositor`.

use std::env;
use std::process::ExitCode;

use rusty_compositor::{parse_args, run_bootstrap};
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn main() -> ExitCode {
    init_tracing();

    let args = env::args().skip(1);
    let config = match parse_args(args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("rusty-compositor error: {err}");
            return ExitCode::from(2);
        }
    };

    match run_bootstrap(&config) {
        Ok(report) => {
            println!(
                "rusty-compositor v{} finished cleanly (mode: {})",
                report.version,
                report.backend.as_str()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("rusty-compositor error: {err}");
            ExitCode::FAILURE
        }
    }
}
