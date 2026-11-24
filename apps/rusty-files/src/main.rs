//! Binary entry point for `rusty-files`, RustyOS's standalone RustyOS file manager
//! (`RUSTYOS-065`, ADR-0030).

mod icons;
mod layout;
pub(crate) mod liquid_glass;
mod render;
mod state;
mod svg_icons;
mod wayland;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rusty_services::file_manager::FilesThemeMode;
use state::{FilePickerMode, StartupOptions};

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
            "--light" => opts.theme_override = Some(FilesThemeMode::Light),
            "--dark" => opts.theme_override = Some(FilesThemeMode::Dark),
            "--open-popup" => opts.open_test_popup = true,
            "--no-gpu" => opts.no_gpu = true,
            "--pick-file" => opts.picker_mode = Some(FilePickerMode::OpenFile),
            "--pick-files" => opts.picker_mode = Some(FilePickerMode::OpenFiles),
            "--pick-folder" => opts.picker_mode = Some(FilePickerMode::OpenFolder),
            "--save-file" => {
                let Some(name) = iter.next() else {
                    return Err(String::from("--save-file requires a default filename"));
                };
                opts.picker_mode = Some(FilePickerMode::SaveFile { default_name: name });
            }
            "--title" => {
                let Some(title) = iter.next() else {
                    return Err(String::from("--title requires a window title argument"));
                };
                opts.picker_title = Some(title);
            }
            "--path" => {
                let Some(dir) = iter.next() else {
                    return Err(String::from("--path requires a directory argument"));
                };
                opts.initial_path = Some(normalize_cli_target_path(&dir));
            }
            other if !other.starts_with('-') && opts.initial_path.is_none() => {
                opts.initial_path = Some(normalize_cli_target_path(other));
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Some(opts))
}

/// Converts a raw CLI path or `file://` URI into a local directory [`PathBuf`].
/// If the target points to an existing regular file, navigates to its parent folder.
fn normalize_cli_target_path(raw: &str) -> PathBuf {
    let decoded = if let Some(rest) = raw.strip_prefix("file://") {
        let path_part = rest.strip_prefix("localhost").unwrap_or(rest);
        percent_decode_uri_path(path_part)
    } else {
        raw.to_owned()
    };
    let candidate = PathBuf::from(decoded);
    if candidate.is_file()
        && let Some(parent) = candidate.parent()
    {
        return parent.to_path_buf();
    }
    candidate
}

fn percent_decode_uri_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex, 16)
        {
            out.push(val);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
                "Usage: rusty-files [--path <dir>] [--trash] [--recents] [--light | --dark] [--no-gpu]"
            );
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("rusty-files error: {msg}");
            return ExitCode::from(2);
        }
    };

    match wayland::run(&opts) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
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
                theme_override: Some(FilesThemeMode::Dark),
                open_test_popup: false,
                no_gpu: false,
                picker_mode: None,
                picker_title: None,
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
                theme_override: Some(FilesThemeMode::Light),
                open_test_popup: false,
                no_gpu: false,
                picker_mode: None,
                picker_title: None,
            }))
        );
        assert_eq!(
            parse_args(["--open-popup".to_owned(), "--no-gpu".to_owned()]),
            Ok(Some(StartupOptions {
                initial_path: None,
                open_trash: false,
                open_recents: false,
                theme_override: None,
                open_test_popup: true,
                no_gpu: true,
                picker_mode: None,
                picker_title: None,
            }))
        );
        assert_eq!(
            parse_args([
                "--save-file".to_owned(),
                "report.pdf".to_owned(),
                "--title".to_owned(),
                "Save Document".to_owned(),
            ]),
            Ok(Some(StartupOptions {
                picker_mode: Some(FilePickerMode::SaveFile {
                    default_name: String::from("report.pdf"),
                }),
                picker_title: Some(String::from("Save Document")),
                ..StartupOptions::default()
            }))
        );
        assert!(parse_args(["--path".to_owned()]).is_err());
    }

    #[test]
    fn normalizes_file_uris_and_resolves_regular_file_to_parent_dir() -> std::io::Result<()> {
        assert_eq!(
            normalize_cli_target_path("file:///tmp/My%20Downloads"),
            PathBuf::from("/tmp/My Downloads")
        );
        let dir = env::temp_dir().join(format!("rusty-files-uri-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("sample.txt");
        std::fs::write(&file, b"ok")?;
        let uri = format!("file://{}", file.display());
        assert_eq!(normalize_cli_target_path(&uri), dir);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
