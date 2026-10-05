//! Checks plugin folders the way LaunchDeck loads them, for plugin authors.
//!
//! `npm run plugins:validate -- <folder>...` from the repository, or
//! `cargo run --example validate_plugin -- <folder>...` from `src-tauri`.
//!
//! Each folder is copied into a temporary plugins folder and scanned by the
//! real plugin host, with grants and secrets kept in memory: the manifest is
//! parsed and validated, the module is loaded and linked, and `init` runs with
//! the default settings and no network. Nothing is written next to the folder,
//! nothing is granted and no secret is read. For each plugin it prints what
//! the Plugins screen shows (the status, the kind of problem and the host's
//! own words) and exits with 1 when any folder fails. A manifest whose `sdk` is
//! older than this LaunchDeck's SDK gets a warning, never a failure.
//!
//! A relative folder is taken from where `npm run` was typed (`INIT_CWD`),
//! otherwise from the current directory.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use launchdeck_lib::plugins::external::manifest::Version;
use launchdeck_lib::plugins::external::{
    Limits, PluginHost, PluginInfo, PluginStatus, ProblemKind, SDK_VERSION,
};
use launchdeck_lib::plugins::PluginRegistry;

const USAGE: &str = "usage: npm run plugins:validate -- <plugin folder>...";

fn main() -> ExitCode {
    let base = std::env::var_os("INIT_CWD").map(PathBuf::from);
    let folders: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(|arg| resolve(base.as_deref(), arg))
        .collect();
    if folders.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut ok = true;
    for folder in &folders {
        println!("{}", folder.display());
        match validate(folder) {
            Ok(passed) => ok &= passed,
            Err(why) => {
                println!("  failed: {why}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn resolve(base: Option<&Path>, arg: OsString) -> PathBuf {
    let path = PathBuf::from(arg);
    match base {
        Some(base) if path.is_relative() => base.join(path),
        _ => path,
    }
}

/// Scan a copy of one folder with the real host. `Ok(false)` when it was
/// scanned and something in it cannot run.
fn validate(folder: &Path) -> Result<bool, String> {
    if !fs::metadata(folder).is_ok_and(|m| m.is_dir()) {
        return Err("not a folder".into());
    }
    let root = tempfile::tempdir().map_err(|e| format!("no temporary folder: {e}"))?;
    let name = folder.file_name().unwrap_or("plugin".as_ref());
    copy_files(folder, &root.path().join(name)).map_err(|e| format!("could not copy it: {e}"))?;

    let host = PluginHost::new(
        vec![root.path().to_path_buf()],
        None,
        PluginRegistry::default(),
        Limits::default(),
    );
    host.scan();
    let overview = host.overview();
    if overview.plugins.is_empty() && overview.issues.is_empty() {
        return Err("there is no plugin.json in it".into());
    }
    let mut passed = true;
    for issue in &overview.issues {
        println!("  skipped, {}", kind_name(issue.kind));
        println!("  {}", issue.error);
        passed = false;
    }
    for info in &overview.plugins {
        passed &= report(info);
    }
    Ok(passed)
}

/// Print one plugin as its card shows it. Whether it can run.
fn report(info: &PluginInfo) -> bool {
    let p = &info.plugin;
    let Some(ext) = &p.external else {
        return false;
    };
    let status = match ext.status {
        PluginStatus::Enabled => "on",
        PluginStatus::Disabled => "disabled",
        PluginStatus::Error => "error",
    };
    match ext.kind {
        Some(kind) => println!(
            "  {} {} ({}): {status}, {}",
            p.id,
            p.version,
            p.name,
            kind_name(kind)
        ),
        None => println!("  {} {} ({}): {status}", p.id, p.version, p.name),
    }
    if let Some(error) = &ext.error {
        println!("  {error}");
    }
    if let Some(warning) = ext.sdk.as_deref().and_then(older_sdk) {
        println!("  warning: {warning}");
    }
    ext.status == PluginStatus::Enabled
}

/// A warning when the manifest says it was built with an older SDK.
fn older_sdk(sdk: &str) -> Option<String> {
    let host = Version::parse(SDK_VERSION)?;
    (Version::parse(sdk)? < host).then(|| {
        format!(
            "built with SDK {sdk}, older than this LaunchDeck's {SDK_VERSION}: still accepted; rebuild with the current SDK to use what it adds"
        )
    })
}

/// The kind as the Plugins screen receives it (`invalidModule`).
fn kind_name(kind: ProblemKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{kind:?}"))
}

/// Copy the files at the top of a plugin folder (the manifest and the module
/// are plain files there). A symbolic link stays a link, so the copy is
/// refused as the folder itself would be.
fn copy_files(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else if kind.is_symlink() {
            copy_link(&entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(from)?, to)
}

/// Without Unix links the copy cannot keep one, and LaunchDeck would refuse it.
#[cfg(not(unix))]
fn copy_link(from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::other(format!(
        "{} is a symbolic link, which LaunchDeck refuses in a plugin folder",
        from.display()
    )))
}
