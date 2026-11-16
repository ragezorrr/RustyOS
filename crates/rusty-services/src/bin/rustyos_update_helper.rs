//! Polkit-authorized privileged helper binary for `rusty-updater` (`RUSTYOS-077`, ADR-0037).
//!
//! Invoked exclusively through `pkexec /usr/lib/rustyos/rustyos-update-helper` with strictly
//! validated non-shell arguments (`sysupgrade` or `install-pkg <abs-path.pkg.tar.*>`).

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match rusty_services::updates::run_helper_cli(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("rustyos-update-helper: {msg}");
            ExitCode::FAILURE
        }
    }
}
