//! Standalone Wayland CSD System Settings window (`RUSTYOS-103`).

mod layout;
pub(crate) mod liquid_glass;
mod render;
mod state;
mod wayland;

use std::env;
use std::process::ExitCode;

use state::{SettingsPage, StartupOptions, ThemeChoice};

fn parse_args<I>(args: I) -> Result<Option<StartupOptions>, String>
where
    I: IntoIterator<Item = String>,
{
    let mut opts = StartupOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--light" => opts.theme_override = Some(ThemeChoice::Light),
            "--dark" => opts.theme_override = Some(ThemeChoice::Dark),
            "--no-gpu" => opts.no_gpu = true,
            "--page" | "-m" => {
                let Some(id) = iter.next() else {
                    return Err(String::from("--page requires a page identifier"));
                };
                opts.initial_page = SettingsPage::from_cli_id(&id);
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
            println!("Usage: rusty-settings [--light | --dark] [--page <id>] [--no-gpu]");
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("rusty-settings error: {msg}");
            return ExitCode::from(2);
        }
    };

    match wayland::run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rusty-settings error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parser_supports_theme_and_page_flags() {
        assert_eq!(
            parse_args([
                "--light".to_owned(),
                "--page".to_owned(),
                "dock".to_owned(),
                "--no-gpu".to_owned(),
            ]),
            Ok(Some(StartupOptions {
                theme_override: Some(ThemeChoice::Light),
                initial_page: SettingsPage::DockDesktop,
                no_gpu: true,
            }))
        );
        assert!(parse_args(["--page".to_owned()]).is_err());
        assert!(parse_args(["--bogus".to_owned()]).is_err());
    }
}
