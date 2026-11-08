use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc_exe() -> Option<PathBuf> {
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(PathBuf::from("rc.exe"));
    }
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = fs::read_dir(kits) {
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("x64").join("rc.exe").exists())
            .collect();
        versions.sort();
        if let Some(latest) = versions.last() {
            return Some(latest.join("x64").join("rc.exe"));
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");

    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        if let Some(rc) = find_rc_exe() {
            let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
            let res_file = out_dir.join("app_icon.res");
            let status = Command::new(rc)
                .args(["/nologo", "/fo"])
                .arg(&res_file)
                .arg("assets/app.rc")
                .status();
            if let Ok(s) = status {
                if s.success() && res_file.exists() {
                    println!("cargo:rustc-link-arg-bins={}", res_file.display());
                }
            }
        }
    }
}
