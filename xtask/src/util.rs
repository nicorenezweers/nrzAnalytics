//! Gemeinsame Hilfen: Fehler, Pfade, Prozesse, git.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Wurzel des Repositorys (Elternordner von `xtask/`).
pub fn root() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    dir.parent().unwrap_or(dir).to_path_buf()
}

/// Build-Ordner ohne Leerzeichen (zig und C-Builds wie SQLite vertragen keine).
pub fn build_dir() -> Result<PathBuf> {
    let dir = std::env::var_os("NRZANALYTICS_BUILD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/nrzanalytics-build"));
    if dir.to_string_lossy().contains(' ') {
        return Err(format!(
            "Build-Ordner {} enthaelt Leerzeichen – NRZANALYTICS_BUILD_DIR auf einen Pfad ohne setzen",
            dir.display()
        )
        .into());
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{} anlegen: {e}", dir.display()))?;
    Ok(dir)
}

/// `version` aus `[workspace.package]` im Root-`Cargo.toml`.
pub fn workspace_version() -> Result<String> {
    let path = root().join("Cargo.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{} lesen: {e}", path.display()))?;
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[workspace.package]";
        } else if in_section && let Some(v) = line.strip_prefix("version") {
            let v = v.trim_start();
            if let Some(v) = v.strip_prefix('=') {
                return Ok(v.trim().trim_matches('"').to_string());
            }
        }
    }
    Err("version in [workspace.package] (Cargo.toml) nicht gefunden".into())
}

/// Grobe Pruefung einer Versionsnummer wie 1.2.3 oder 1.2.3-rc.1.
pub fn check_version(v: &str) -> Result<()> {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(format!("'{v}' ist keine Versionsnummer (erwartet z. B. 1.0.1)").into())
    }
}

pub fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

pub fn cmd<S: AsRef<OsStr>>(program: S, args: &[&str]) -> Command {
    let mut c = Command::new(program);
    c.args(args);
    c
}

/// Befehl ausfuehren (Ausgabe durchreichen); Fehler bei Exit != 0.
pub fn run(c: &mut Command, what: &str) -> Result<()> {
    let status = c
        .status()
        .map_err(|e| format!("{what}: {:?} nicht startbar: {e}", c.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} fehlgeschlagen ({status})").into())
    }
}

/// Befehl ausfuehren und stdout (getrimmt) zurueckgeben.
pub fn output(c: &mut Command, what: &str) -> Result<String> {
    let out = c
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{what}: {:?} nicht startbar: {e}", c.get_program()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "{what} fehlgeschlagen: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into())
    }
}

/// Befehl leise ausfuehren, nur Erfolg zaehlt.
pub fn succeeds(c: &mut Command) -> bool {
    c.stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn git(args: &[&str]) -> Command {
    let mut c = cmd("git", args);
    c.current_dir(root());
    c
}

pub fn git_in(dir: &Path, args: &[&str]) -> Command {
    let mut c = cmd("git", args);
    c.current_dir(dir);
    c
}

/// Arbeitsbaum ohne Aenderungen (auch ohne unversionierte Dateien)?
pub fn ensure_clean() -> Result<()> {
    let status = output(&mut git(&["status", "--porcelain"]), "git status")?;
    if status.is_empty() {
        Ok(())
    } else {
        Err(format!("Arbeitsverzeichnis nicht sauber – erst committen:\n{status}").into())
    }
}

pub fn local_tag_exists(tag: &str) -> bool {
    succeeds(&mut git(&[
        "rev-parse",
        "-q",
        "--verify",
        &format!("refs/tags/{tag}"),
    ]))
}

/// Gibt es den Tag auf origin? `None`, wenn origin nicht erreichbar ist.
pub fn remote_tag_exists(tag: &str) -> Option<bool> {
    output(
        &mut git(&["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")]),
        "git ls-remote",
    )
    .ok()
    .map(|s| !s.is_empty())
}

/// Ein Programm im PATH suchen.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Eindeutiger temporaerer Ordner (wird vom Aufrufer entfernt).
pub fn temp_dir(prefix: &str) -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("{} anlegen: {e}", dir.display()))?;
    Ok(dir)
}

/// Entfernt einen Ordner beim Verlassen des Gueltigkeitsbereichs (auch bei Fehlern).
pub struct RemoveOnDrop(pub PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// SHA-256 als Hex.
pub fn sha256_hex(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| format!("{} lesen: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `SHA256SUMS` im Format von `shasum -a 256` (`<hex>  <datei>`) fuer die genannten Dateien.
pub fn write_sha256sums(dir: &Path, files: &[String]) -> Result<()> {
    let mut text = String::new();
    for f in files {
        text.push_str(&format!("{}  {f}\n", sha256_hex(&dir.join(f))?));
    }
    let path = dir.join("SHA256SUMS");
    fs::write(&path, text).map_err(|e| format!("{} schreiben: {e}", path.display()).into())
}

/// Datei mit gzip -9 packen.
pub fn gzip(src: &Path, dst: &Path) -> Result<()> {
    let mut input = fs::File::open(src).map_err(|e| format!("{} lesen: {e}", src.display()))?;
    let out = fs::File::create(dst).map_err(|e| format!("{} schreiben: {e}", dst.display()))?;
    let mut enc = flate2::write::GzEncoder::new(out, flate2::Compression::best());
    std::io::copy(&mut input, &mut enc)?;
    enc.finish()?;
    Ok(())
}

/// Groesse lesbar (MiB).
pub fn size(path: &Path) -> String {
    fs::metadata(path)
        .map(|m| format!("{:.1} MiB", m.len() as f64 / 1024.0 / 1024.0))
        .unwrap_or_else(|_| "?".into())
}
