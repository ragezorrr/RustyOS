//! Binary entry point for `rusty-files`, RustyOS's standalone Golden Gate file manager
//! (`RUSTYOS-065`, ADR-0030).

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
            "--trash" => opts.open_trash = true,
            "--recents" => opts.open_recents = true,
            "--light" => opts.theme_override = Some(FilesThemeMode::LightGoldenGate),
            "--dark" => opts.theme_override = Some(FilesThemeMode::DarkPurpleGlass),
            "--path" => {
                let Some(dir) = iter.next() else {
                    return Err(String::from("--path requires a directory argument"));
                };
                opts.initial_path = Some(PathBuf::from(dir));
            }
            other if !other.starts_with('-') && opts.initial_path.is_none() => {
                opts.initial_path = Some(PathBuf::from(other));
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
            println!("Usage: rusty-files [--path <dir>] [--trash] [--recents] [--light | --dark]");
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("rusty-files error: {msg}");
            return ExitCode::from(2);
        }
    };

    match wayland::run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rusty-files error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parser_supports_flags_and_positional_path() {
        assert_eq!(
            parse_args(["--trash".to_owned(), "--dark".to_owned()]),
            Ok(Some(StartupOptions {
                initial_path: None,
                open_trash: true,
                open_recents: false,
                theme_override: Some(FilesThemeMode::DarkPurpleGlass),
            }))
        );
        assert_eq!(
            parse_args([
                "--path".to_owned(),
                "/tmp/demo".to_owned(),
                "--light".to_owned()
            ]),
            Ok(Some(StartupOptions {
                initial_path: Some(PathBuf::from("/tmp/demo")),
                open_trash: false,
                open_recents: false,
                theme_override: Some(FilesThemeMode::LightGoldenGate),
            }))
        );
        assert!(parse_args(["--path".to_owned()]).is_err());
    }
}
