//! Compiles the isolated `rusty-pipewire-stream` C worker when `libpipewire-0.3`
//! development headers and a C compiler are available (`RUSTYOS-057`, ADR-0026).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=c/rusty-pipewire-stream.c");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=CC");

    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    let out_bin = PathBuf::from(&out_dir).join("rusty-pipewire-stream");

    let Ok(cflags_out) = Command::new("pkg-config")
        .args(["--cflags", "libpipewire-0.3"])
        .output()
    else {
        println!("cargo:warning=pkg-config not found; rusty-pipewire-stream skipped");
        return;
    };
    let Ok(libs_out) = Command::new("pkg-config")
        .args(["--libs", "libpipewire-0.3"])
        .output()
    else {
        println!("cargo:warning=pkg-config not found; rusty-pipewire-stream skipped");
        return;
    };
    if !cflags_out.status.success() || !libs_out.status.success() {
        println!(
            "cargo:warning=libpipewire-0.3 development files unavailable; ScreenCast helper disabled at build time"
        );
        return;
    }

    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let mut cmd = Command::new(&cc);
    cmd.args([
        "-O2",
        "-std=c11",
        "-Wall",
        "-Wextra",
        "-pthread",
        "c/rusty-pipewire-stream.c",
        "-o",
    ]);
    cmd.arg(&out_bin);
    for flag in String::from_utf8_lossy(&cflags_out.stdout).split_whitespace() {
        cmd.arg(flag);
    }
    for flag in String::from_utf8_lossy(&libs_out.stdout).split_whitespace() {
        cmd.arg(flag);
    }

    match cmd.status() {
        Ok(status) if status.success() => {
            println!(
                "cargo:rustc-env=RUSTY_PIPEWIRE_STREAM_BIN={}",
                out_bin.display()
            );
        }
        Ok(status) => {
            println!("cargo:warning=failed to compile c/rusty-pipewire-stream.c (exit {status})");
            std::process::exit(1);
        }
        Err(err) => {
            println!(
                "cargo:warning=C compiler {cc:?} unavailable ({err}); rusty-pipewire-stream skipped"
            );
        }
    }
}
