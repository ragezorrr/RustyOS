#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::env;
use std::fs::File;
use std::path::PathBuf;

use gl_generator::{Api, Fallbacks, Profile, Registry, StructGenerator};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let dest = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR missing"));

    let mut egl_file = File::create(dest.join("egl_bindings.rs")).expect("create egl_bindings.rs");
    Registry::new(
        Api::Egl,
        (1, 5),
        Profile::Core,
        Fallbacks::All,
        [
            "EGL_EXT_platform_base",
            "EGL_EXT_platform_wayland",
            "EGL_KHR_platform_wayland",
        ],
    )
    .write_bindings(StructGenerator, &mut egl_file)
    .expect("write egl bindings");

    let mut gl_file = File::create(dest.join("gl_bindings.rs")).expect("create gl_bindings.rs");
    Registry::new(
        Api::Gles2,
        (2, 0),
        Profile::Core,
        Fallbacks::All,
        ["GL_EXT_texture_format_BGRA8888", "GL_EXT_unpack_subimage"],
    )
    .write_bindings(StructGenerator, &mut gl_file)
    .expect("write gles2 bindings");
}
