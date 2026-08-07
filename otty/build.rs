//! Build script: embeds the application icon into the Windows executable.
//!
//! Non-Windows targets are unaffected. On Windows the resource script
//! `assets/win/otty.rc` is compiled with the toolchain's resource compiler
//! (`rc.exe` for MSVC, `windres` for GNU) and linked into the binary.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let rc_file = manifest_dir.join("../assets/win/otty.rc");
    let rc_dir = rc_file.parent().unwrap().to_path_buf();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    println!("cargo:rerun-if-changed={}", rc_file.display());
    println!(
        "cargo:rerun-if-changed={}",
        rc_dir.join("otty.ico").display()
    );

    let output = if env == "msvc" {
        let res = out_dir.join("otty.res");
        run(Command::new("rc.exe")
            .arg("/fo")
            .arg(&res)
            .arg("/i")
            .arg(&rc_dir)
            .arg(&rc_file));
        res
    } else {
        let obj = out_dir.join("otty-rc.o");
        run(Command::new("x86_64-w64-mingw32-windres")
            .arg("-i")
            .arg(&rc_file)
            .arg("-I")
            .arg(&rc_dir)
            .arg("-o")
            .arg(&obj));
        obj
    };

    println!("cargo:rustc-link-arg={}", output.display());
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|err| panic!("failed to run {command:?}: {err}"));
    assert!(status.success(), "resource compiler failed: {command:?}");
}
