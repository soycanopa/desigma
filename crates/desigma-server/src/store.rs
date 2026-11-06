//! Loading, atomic saving and rotating backups of the served document.

use desigma_doc::Document;
use desigma_library::{load_external, write_atomic};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Opens `input` and says where the document lives from now on, with a log line.
///
/// * `.desigma` that exists: loaded. One that does not exist yet starts as a new empty document.
/// * `.fig`, `.pen`, REST `.json`, `.svg`: imported and saved beside it as `<name>.desigma`. Once that
///   file exists it wins (so restarting with the same command keeps the edits made since).
pub fn load_document(input: &Path) -> Result<(PathBuf, Document, String), String> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled");
    let target = if ext == "desigma" { input.to_path_buf() } else { input.with_extension("desigma") };
    if target.exists() {
        let doc = load_external(&target).map_err(|e| format!("cannot open {}: {e}", target.display()))?;
        let note = if target == input {
            format!("serving {}", target.display())
        } else {
            format!("serving {} (saved earlier from {}; delete it to import again)", target.display(), input.display())
        };
        return Ok((target, doc, note));
    }
    if ext == "desigma" && !input.exists() {
        let mut doc = Document::new(stem);
        doc.add_page("Page 1");
        return Ok((target, doc, format!("{} does not exist, starting a new document", input.display())));
    }
    let doc = load_external(input).map_err(|e| format!("cannot import {}: {e}", input.display()))?;
    Ok((target.clone(), doc, format!("imported {}, will save as {}", input.display(), target.display())))
}

/// Writes `bytes` to a temporary file next to `path`, then renames it over `path`.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn backup(path: &Path, n: usize) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(format!(".bak.{n}"));
    PathBuf::from(p)
}

/// Before the file is overwritten: keeps up to `keep` earlier copies (`.bak.1` newest), taking a
/// new one only when the newest is older than `every`. Best effort: a failed backup never stops a save.
pub fn rotate_backups(path: &Path, keep: usize, every: Duration) {
    if keep == 0 || !path.exists() {
        return;
    }
    let age = std::fs::metadata(backup(path, 1)).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if age.is_some_and(|a| a < every) {
        return;
    }
    let _ = std::fs::remove_file(backup(path, keep));
    for n in (1..keep).rev() {
        let _ = std::fs::rename(backup(path, n), backup(path, n + 1));
    }
    let _ = std::fs::copy(path, backup(path, 1));
}

/// `2026-10-25 12:00:00 UTC`, without a date library.
pub fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // civil-from-days (Howard Hinnant)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("desigma-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn backups_rotate_and_are_capped() {
        let d = dir("bak");
        let f = d.join("a.desigma");
        for i in 0..5 {
            std::fs::write(&f, format!("v{i}")).unwrap();
            rotate_backups(&f, 2, Duration::ZERO);
        }
        assert_eq!(std::fs::read_to_string(backup(&f, 1)).unwrap(), "v4");
        assert_eq!(std::fs::read_to_string(backup(&f, 2)).unwrap(), "v3");
        assert!(!backup(&f, 3).exists());
        // Too soon for another one.
        std::fs::write(&f, "v5").unwrap();
        rotate_backups(&f, 2, Duration::from_secs(3600));
        assert_eq!(std::fs::read_to_string(backup(&f, 1)).unwrap(), "v4");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_missing_desigma_file_starts_empty_and_a_saved_import_wins() {
        let d = dir("load");
        let (p, doc, _) = load_document(&d.join("new.desigma")).unwrap();
        assert_eq!(doc.pages.len(), 1);
        save_atomic(&p, doc.to_json().unwrap().as_bytes()).unwrap();
        assert!(!d.join("new.desigma.tmp").exists());
        // `new.json` was never written, but `new.desigma` exists, so it is what gets served.
        let (p2, _, note) = load_document(&d.join("new.json")).unwrap();
        assert_eq!(p2, d.join("new.desigma"));
        assert!(note.contains("saved earlier"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn timestamp_looks_right() {
        let t = utc_now();
        assert_eq!(t.len(), 23, "{t}");
        assert!(t.starts_with("20"));
    }
}
