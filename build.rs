//! Embeds assets/icon.ico into the Windows executable (Explorer, taskbar, Alt+Tab) using the
//! Windows SDK's rc.exe directly instead of a build-dependency crate. Without rc.exe the build
//! still succeeds, just without the icon.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe not found (install the Windows SDK or set RC); building without an exe icon");
        return;
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let ico = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("icon.ico");
    let rc_file = out.join("icon.rc");
    let res_file = out.join("icon.res");
    // Resource id 1: the shell uses the lowest-numbered icon as the exe icon.
    std::fs::write(&rc_file, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "\\\\")))
        .expect("write icon.rc");
    let status = Command::new(&rc).arg("/nologo").arg("/fo").arg(&res_file).arg(&rc_file).status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res_file.display()),
        other => println!("cargo:warning=rc.exe failed ({other:?}); building without an exe icon"),
    }
}

/// `RC` env var, else the newest x64 rc.exe under the Windows 10/11 SDK.
fn find_rc() -> Option<PathBuf> {
    if let Some(rc) = std::env::var_os("RC") {
        return Some(rc.into());
    }
    let kits = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits").join("10").join("bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path().join("x64").join("rc.exe")))
        .filter(|p| p.is_file())
        .collect();
    // Version directories (10.0.x.y) sort correctly as strings within the same major/minor.
    versions.sort();
    versions.pop()
}
