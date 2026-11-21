//! Binary entry point for `rusty-installer`, RustyOS's macOS-style OS Installer & OOBE Setup Assistant
//! (`RUSTYOS-115`, ADR-0065).

mod disks;
mod layout;
mod liquid_glass;
mod render;
mod state;
mod wayland;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rusty_services::file_manager::{FilesThemeMode, clamp_accent_index};
use state::{RunEnv, StartupOptions, WizardMode};

fn parse_args<I>(args: I) -> Result<Option<StartupOptions>, String>
where
    I: IntoIterator<Item = String>,
{
    let mut opts = StartupOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--install" => opts.mode = WizardMode::Install,
            "--oobe" => opts.mode = WizardMode::Oobe,
            "--live" => {
                opts.mode = WizardMode::Install;
                opts.env_kind = RunEnv::Live;
            }
            "--demo" => opts.env_kind = RunEnv::Demo,
            "--dry-run" => opts.dry_run = true,
            "--no-glass" => opts.no_glass = true,
            "--no-gpu" => opts.no_gpu = true,
            "--light" => opts.theme_override = Some(FilesThemeMode::Light),
            "--dark" => opts.theme_override = Some(FilesThemeMode::Dark),
            "--accent" => {
                let Some(raw) = iter.next() else {
                    return Err(String::from("--accent requires a numeric index 0..6"));
                };
                let parsed = raw
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --accent index '{raw}'"))?;
                opts.accent_override = Some(clamp_accent_index(parsed));
            }
            "--install-script" => {
                let Some(path) = iter.next() else {
                    return Err(String::from("--install-script requires a path"));
                };
                opts.install_script = Some(PathBuf::from(path));
            }
            "--step" => {
                let Some(raw) = iter.next() else {
                    return Err(String::from("--step requires a step name"));
                };
                opts.initial_step = state::InstallStep::from_cli_name(&raw);
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
                "Usage: rusty-installer [--install | --oobe] [--live] [--demo] [--dry-run] [--light | --dark] [--accent <0..6>] [--no-glass] [--no-gpu] [--install-script <path>]"
            );
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("rusty-installer error: {msg}");
            return ExitCode::from(2);
        }
    };

    match wayland::run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rusty-installer error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parser_supports_install_oobe_dry_run_and_theme_flags() {
        assert_eq!(
            parse_args([
                "--oobe".to_owned(),
                "--dark".to_owned(),
                "--accent".to_owned(),
                "2".to_owned(),
                "--dry-run".to_owned(),
                "--demo".to_owned(),
                "--no-glass".to_owned(),
            ]),
            Ok(Some(StartupOptions {
                mode: WizardMode::Oobe,
                env_kind: RunEnv::Demo,
                dry_run: true,
                no_glass: true,
                theme_override: Some(FilesThemeMode::Dark),
                accent_override: Some(2),
                ..Default::default()
            }))
        );
        assert!(parse_args(["--accent".to_owned()]).is_err());
        assert!(parse_args(["--accent".to_owned(), "abc".to_owned()]).is_err());
        assert!(parse_args(["--bogus".to_owned()]).is_err());
    }
}
