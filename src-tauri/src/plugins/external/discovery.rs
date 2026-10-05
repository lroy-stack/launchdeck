//! Finding plugins on disk.
//!
//! A plugin is a directory holding `plugin.json` and the module it names. The
//! scan never executes anything and never fails as a whole: a broken directory
//! becomes an [`Issue`] and the rest are still found. Files are read with size
//! caps and symbolic links are refused, so a plugin directory cannot make the
//! host read something outside itself.

use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::manifest::{ExternalManifest, MANIFEST_FILE, MAX_MANIFEST_BYTES};
use super::ProblemKind;

/// Largest `.wasm` module the host will read.
pub const MAX_MODULE_BYTES: usize = 8 * 1024 * 1024;

/// One plugin directory that passed manifest validation.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub dir: PathBuf,
    pub manifest: ExternalManifest,
    pub wasm: Vec<u8>,
    /// Changes whenever the manifest or the module changes. Lets a rescan keep
    /// an unchanged plugin loaded (and its state) instead of restarting it.
    pub fingerprint: u64,
}

/// One plugin directory that could not be used, and why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub dir: String,
    pub kind: ProblemKind,
    pub error: String,
}

/// Why a plugin directory could not be loaded: what kind of problem, and the
/// reason in words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct LoadError {
    pub kind: ProblemKind,
    pub message: String,
}

impl LoadError {
    fn new(kind: ProblemKind, message: impl Into<String>) -> LoadError {
        LoadError {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
}

/// Read a regular file (not a symlink) of at most `max` bytes.
pub(super) fn read_plain_file(path: &Path, max: usize, what: &str) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("cannot read {what}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{what} is a symbolic link, which is not allowed"));
    }
    if !meta.is_file() {
        return Err(format!("{what} is not a regular file"));
    }
    if meta.len() > max as u64 {
        return Err(format!("{what} is larger than {max} bytes"));
    }
    let file = fs::File::open(path).map_err(|e| format!("cannot open {what}: {e}"))?;
    // The length above can change between the check and the read; cap the read too.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {what}: {e}"))?;
    if bytes.len() > max {
        return Err(format!("{what} is larger than {max} bytes"));
    }
    Ok(bytes)
}

/// Load and validate one plugin directory.
pub fn load_dir(dir: &Path) -> Result<Candidate, LoadError> {
    let bad_manifest = |e| LoadError::new(ProblemKind::InvalidManifest, e);
    let bad_module = |e| LoadError::new(ProblemKind::InvalidModule, e);
    let manifest_bytes =
        read_plain_file(&dir.join(MANIFEST_FILE), MAX_MANIFEST_BYTES, MANIFEST_FILE)
            .map_err(bad_manifest)?;
    let text = std::str::from_utf8(&manifest_bytes)
        .map_err(|_| bad_manifest(format!("{MANIFEST_FILE} is not valid UTF-8")))?;
    let manifest =
        ExternalManifest::parse(text).map_err(|e| LoadError::new(e.kind(), e.to_string()))?;
    let wasm = read_plain_file(
        &dir.join(&manifest.main),
        MAX_MODULE_BYTES,
        &format!("module `{}`", manifest.main),
    )
    .map_err(bad_module)?;
    if !wasm.starts_with(b"\0asm") {
        return Err(bad_module(format!(
            "`{}` is not a WebAssembly module",
            manifest.main
        )));
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    manifest_bytes.hash(&mut hasher);
    wasm.hash(&mut hasher);
    Ok(Candidate {
        dir: dir.to_path_buf(),
        manifest,
        wasm,
        fingerprint: hasher.finish(),
    })
}

/// Scan every root for plugin directories. Roots are searched in order and
/// directories by name, so the result is deterministic. When two directories
/// carry the same plugin id the first one wins and the other is reported.
pub fn scan(roots: &[PathBuf]) -> Scan {
    let mut out = Scan::default();
    let mut owners: HashMap<String, PathBuf> = HashMap::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue; // A missing plugins folder is normal.
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            // A folder without a manifest is not a plugin; stay quiet about it.
            if fs::symlink_metadata(dir.join(MANIFEST_FILE)).is_err() {
                continue;
            }
            match load_dir(&dir) {
                Ok(candidate) => {
                    if let Some(first) = owners.get(&candidate.manifest.id) {
                        out.issues.push(Issue {
                            dir: dir.display().to_string(),
                            kind: ProblemKind::Duplicate,
                            error: format!(
                                "duplicate plugin id `{}`; already provided by {}",
                                candidate.manifest.id,
                                first.display()
                            ),
                        });
                    } else {
                        owners.insert(candidate.manifest.id.clone(), dir.clone());
                        out.candidates.push(candidate);
                    }
                }
                Err(e) => out.issues.push(Issue {
                    dir: dir.display().to_string(),
                    kind: e.kind,
                    error: e.message,
                }),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn manifest(id: &str, main: &str) -> String {
        json!({"schemaVersion":1,"id":id,"name":"T","version":"1.0.0","author":"t","abi":1,"main":main})
            .to_string()
    }

    pub(crate) fn write_plugin(root: &Path, dir: &str, manifest: &str, wasm: &[u8]) -> PathBuf {
        let d = root.join(dir);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(MANIFEST_FILE), manifest).unwrap();
        fs::write(d.join("m.wasm"), wasm).unwrap();
        d
    }

    const WASM: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn finds_valid_plugins_and_reports_broken_ones() {
        let root = tempfile::tempdir().unwrap();
        write_plugin(root.path(), "good", &manifest("a.good", "m.wasm"), WASM);
        write_plugin(root.path(), "badjson", "{nope", WASM);
        write_plugin(
            root.path(),
            "badwasm",
            &manifest("a.nowasm", "m.wasm"),
            b"MZ....",
        );
        write_plugin(root.path(), "badid", &manifest("Nope", "m.wasm"), WASM);
        fs::create_dir_all(root.path().join("not-a-plugin")).unwrap();
        fs::write(root.path().join("stray.txt"), "x").unwrap();
        let s = scan(&[root.path().to_path_buf()]);
        assert_eq!(s.candidates.len(), 1);
        assert_eq!(s.candidates[0].manifest.id, "a.good");
        assert_eq!(s.issues.len(), 3, "{:?}", s.issues);
    }

    #[test]
    fn each_unusable_folder_says_what_kind_of_problem_it_has() {
        use crate::plugins::external::ProblemKind;
        let root = tempfile::tempdir().unwrap();
        let future = json!({"schemaVersion":99,"id":"a.future","name":"T","version":"1.0.0",
            "author":"t","abi":1,"main":"m.wasm"});
        write_plugin(root.path(), "future", &future.to_string(), WASM);
        write_plugin(root.path(), "badjson", "{nope", WASM);
        write_plugin(root.path(), "badid", &manifest("Nope", "m.wasm"), WASM);
        write_plugin(
            root.path(),
            "badwasm",
            &manifest("a.nowasm", "m.wasm"),
            b"MZ..",
        );
        write_plugin(
            root.path(),
            "nomodule",
            &manifest("a.nomod", "x.wasm"),
            WASM,
        );
        let s = scan(&[root.path().to_path_buf()]);
        let kind = |dir: &str| {
            s.issues
                .iter()
                .find(|i| i.dir.ends_with(dir))
                .unwrap_or_else(|| panic!("{dir}: {:?}", s.issues))
                .kind
        };
        assert_eq!(kind("future"), ProblemKind::Incompatible);
        assert_eq!(kind("badjson"), ProblemKind::InvalidManifest);
        assert_eq!(kind("badid"), ProblemKind::InvalidManifest);
        assert_eq!(kind("badwasm"), ProblemKind::InvalidModule);
        assert_eq!(kind("nomodule"), ProblemKind::InvalidModule);
        // The kind is decided here; the detail stays the readable reason.
        let e = load_dir(&root.path().join("badjson")).unwrap_err();
        assert_eq!(e.kind, ProblemKind::InvalidManifest);
        assert!(e.to_string().contains("not valid JSON"), "{e}");
    }

    #[test]
    fn missing_roots_are_fine() {
        let s = scan(&[PathBuf::from("/definitely/not/here")]);
        assert!(s.candidates.is_empty() && s.issues.is_empty());
    }

    #[test]
    fn duplicate_ids_keep_the_first_and_report_the_second() {
        let root = tempfile::tempdir().unwrap();
        write_plugin(root.path(), "a-first", &manifest("a.same", "m.wasm"), WASM);
        write_plugin(root.path(), "b-second", &manifest("a.same", "m.wasm"), WASM);
        let s = scan(&[root.path().to_path_buf()]);
        assert_eq!(s.candidates.len(), 1);
        assert!(s.candidates[0].dir.ends_with("a-first"));
        assert_eq!(s.issues.len(), 1);
        assert!(s.issues[0].error.contains("duplicate plugin id"));
    }

    #[test]
    fn duplicates_across_roots_prefer_the_earlier_root() {
        let r1 = tempfile::tempdir().unwrap();
        let r2 = tempfile::tempdir().unwrap();
        write_plugin(r1.path(), "x", &manifest("a.same", "m.wasm"), WASM);
        write_plugin(r2.path(), "y", &manifest("a.same", "m.wasm"), WASM);
        let s = scan(&[r1.path().to_path_buf(), r2.path().to_path_buf()]);
        assert_eq!(s.candidates.len(), 1);
        assert!(s.candidates[0].dir.starts_with(r1.path()));
        assert_eq!(s.issues.len(), 1);
        assert_eq!(
            s.issues[0].kind,
            crate::plugins::external::ProblemKind::Duplicate
        );
    }

    #[test]
    fn oversized_files_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let big = vec![0u8; MAX_MODULE_BYTES + 1];
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        wasm.extend(big);
        write_plugin(root.path(), "big", &manifest("a.big", "m.wasm"), &wasm);
        let huge_manifest = format!("{{\"x\":\"{}\"}}", "a".repeat(MAX_MANIFEST_BYTES));
        write_plugin(root.path(), "hugem", &huge_manifest, WASM);
        let s = scan(&[root.path().to_path_buf()]);
        assert!(s.candidates.is_empty());
        assert_eq!(s.issues.len(), 2, "{:?}", s.issues);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.wasm"), WASM).unwrap();
        let d = root.path().join("linky");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(MANIFEST_FILE), manifest("a.linky", "m.wasm")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.wasm"), d.join("m.wasm")).unwrap();
        let s = scan(&[root.path().to_path_buf()]);
        assert!(s.candidates.is_empty());
        assert!(
            s.issues[0].error.contains("symbolic link"),
            "{:?}",
            s.issues
        );
    }

    #[test]
    fn fingerprint_changes_with_content() {
        let root = tempfile::tempdir().unwrap();
        let d = write_plugin(root.path(), "p", &manifest("a.p", "m.wasm"), WASM);
        let a = load_dir(&d).unwrap().fingerprint;
        assert_eq!(a, load_dir(&d).unwrap().fingerprint);
        let mut changed = WASM.to_vec();
        changed.push(0);
        fs::write(d.join("m.wasm"), changed).unwrap();
        assert_ne!(a, load_dir(&d).unwrap().fingerprint);
    }
}
