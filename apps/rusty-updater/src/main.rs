//! Binary entry point for `rusty-updater`, RustyOS's standalone Golden Gate Software Update UI
//! (`RUSTYOS-077`, ADR-0037).

mod layout;
mod render;
mod state;
mod wayland;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rusty_services::file_manager::FilesThemeMode;
use state::StartupOptions;

fn parse_args<I>(args: I) -> Result<Option<StartupOptions>, String>
where
    I: IntoIterator<Item = String>,
{
    let mut opts = StartupOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--light" => opts.theme_override = Some(FilesThemeMode::LightGoldenGate),
            "--dark" => opts.theme_override = Some(FilesThemeMode::DarkPurpleGlass),
            "--dry-run" => opts.dry_run = true,
            "--check-now" => opts.check_immediately = true,
            "--pkgbuild-dir" => {
                let Some(dir) = iter.next() else {
                    return Err(String::from("--pkgbuild-dir requires a directory path"));
                };
                opts.pkgbuild_dir = Some(PathBuf::from(dir));
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Some(opts))
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let opts = match parse_args(env::args().skip(1)) {
        Ok(Some(o)) => o,
        Ok(None) => {
            println!(
                "Usage: rusty-updater [--light | --dark] [--dry-run] [--check-now] [--pkgbuild-dir <dir>]"
            );
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("rusty-updater error: {msg}");
            return ExitCode::from(2);
        }
    };

    match wayland::run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rusty-updater error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parser_supports_theme_dry_run_and_pkgbuild_flags() {
        assert_eq!(
            parse_args([
                "--dark".to_owned(),
                "--dry-run".to_owned(),
                "--check-now".to_owned(),
                "--pkgbuild-dir".to_owned(),
                "/tmp/arch-pkg".to_owned(),
            ]),
            Ok(Some(StartupOptions {
                theme_override: Some(FilesThemeMode::DarkPurpleGlass),
                dry_run: true,
                check_immediately: true,
                pkgbuild_dir: Some(PathBuf::from("/tmp/arch-pkg")),
            }))
        );
        assert!(parse_args(["--pkgbuild-dir".to_owned()]).is_err());
        assert!(parse_args(["--bogus".to_owned()]).is_err());
    }
}
