//! Release-Programme: Oberflaeche einbetten, Linux (musl, statisch) per cargo-zigbuild, macOS nativ.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    util::{self, Result, root, run},
    web,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    LinuxAmd64,
    LinuxArm64,
    MacosArm64,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::LinuxAmd64, Target::LinuxArm64, Target::MacosArm64];

    pub fn name(self) -> &'static str {
        match self {
            Target::LinuxAmd64 => "linux-amd64",
            Target::LinuxArm64 => "linux-arm64",
            Target::MacosArm64 => "macos-arm64",
        }
    }

    fn triple(self) -> &'static str {
        match self {
            Target::LinuxAmd64 => "x86_64-unknown-linux-musl",
            Target::LinuxArm64 => "aarch64-unknown-linux-musl",
            Target::MacosArm64 => "aarch64-apple-darwin",
        }
    }

    /// Architektur-Name fuer Umbrel (`nrzanalytics-<arch>.gz`), nur Linux.
    pub fn umbrel_arch(self) -> Option<&'static str> {
        match self {
            Target::LinuxAmd64 => Some("amd64"),
            Target::LinuxArm64 => Some("arm64"),
            Target::MacosArm64 => None,
        }
    }
}

pub fn parse_targets(args: &[String]) -> Result<Vec<Target>> {
    let mut value = "all";
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--target" => value = it.next().ok_or("dist: --target braucht einen Wert")?,
            v if v.starts_with("--target=") => value = &v["--target=".len()..],
            other => return Err(format!("dist: unbekanntes Argument '{other}'").into()),
        }
    }
    if value == "all" {
        return Ok(Target::ALL.to_vec());
    }
    let mut targets = Vec::new();
    for v in value.split(',') {
        let t = Target::ALL.into_iter().find(|t| t.name() == v).ok_or_else(|| {
            format!("dist: unbekanntes Ziel '{v}' (linux-amd64, linux-arm64, macos-arm64, all)")
        })?;
        if !targets.contains(&t) {
            targets.push(t);
        }
    }
    Ok(targets)
}

/// Baut die Ziele nach `target/dist/<version>/` und gibt den Ordner zurueck.
pub fn dist(targets: &[Target]) -> Result<PathBuf> {
    let version = util::workspace_version()?;
    let root = root();
    let build = util::build_dir()?;
    let out = root.join("target/dist").join(&version);
    fs::create_dir_all(&out).map_err(|e| format!("{} anlegen: {e}", out.display()))?;

    let linux = targets.iter().any(|t| t.umbrel_arch().is_some());
    if linux {
        preflight_zig()?;
    }
    if targets.contains(&Target::MacosArm64) && !(cfg!(target_os = "macos") && cfg!(target_arch = "aarch64"))
    {
        return Err("macos-arm64 laesst sich nur auf einem Mac mit Apple Silicon bauen".into());
    }

    let public = web::bundle(&build)?;
    for &t in targets {
        let bin = build_server(t, &build, &public)?;
        check_binary(t, &bin)?;
        package(t, &bin, &out, &version)?;
    }

    // SHA256SUMS ueber alle Pakete im Ordner (auch die frueherer Laeufe mit anderem --target).
    let mut files: Vec<String> = fs::read_dir(&out)?
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != "SHA256SUMS" && !n.starts_with('.'))
        .collect();
    files.sort();
    util::write_sha256sums(&out, &files)?;
    println!("==> Inhalt von {}:", out.display());
    for f in &files {
        println!("    {f:<44} {}", util::size(&out.join(f)));
    }
    Ok(out)
}

fn preflight_zig() -> Result<()> {
    if util::which("zig").is_none() {
        return Err("zig nicht gefunden – brew install zig (wird von cargo-zigbuild gebraucht)".into());
    }
    if !util::succeeds(&mut util::cmd(util::cargo(), &["zigbuild", "--help"])) {
        return Err("cargo-zigbuild fehlt – cargo install --locked cargo-zigbuild".into());
    }
    Ok(())
}

/// Schreibt `zig ar`/`zig ranlib`-Wrapper: Apples `ar` kann keine ELF-Objekte und erzeugt sonst
/// leere Archive (z. B. fuer SQLite). Die Wrapper landen nur im Build-Ordner.
fn zig_wrappers(build: &Path) -> Result<(PathBuf, PathBuf)> {
    let bin = build.join("bin");
    fs::create_dir_all(&bin)?;
    let mut paths = Vec::new();
    for tool in ["ar", "ranlib"] {
        let path = bin.join(format!("zig-{tool}"));
        fs::write(&path, format!("#!/bin/sh\nexec zig {tool} \"$@\"\n"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
        paths.push(path);
    }
    Ok((paths[0].clone(), paths[1].clone()))
}

fn build_server(t: Target, build: &Path, public: &Path) -> Result<PathBuf> {
    println!("==> Server {} ({})", t.name(), t.triple());
    let cargo = util::cargo();
    let (mut c, target_dir) = if t.umbrel_arch().is_some() {
        let (ar, ranlib) = zig_wrappers(build)?;
        let target_dir = build.join("linux");
        let mut c = util::cmd(
            &cargo,
            &[
                "zigbuild",
                "--release",
                "-p",
                "nrzanalytics",
                "--target",
                t.triple(),
            ],
        );
        let env_triple = t.triple().replace('-', "_");
        c.env("CARGO_ZIGBUILD_CACHE_DIR", build.join("zigcache"))
            .env("AR", &ar)
            .env("RANLIB", &ranlib)
            .env(format!("AR_{env_triple}"), &ar)
            .env(format!("RANLIB_{env_triple}"), &ranlib);
        (c, target_dir)
    } else {
        let target_dir = build.join("macos");
        (
            util::cmd(
                &cargo,
                &["build", "--release", "-p", "nrzanalytics", "--target", t.triple()],
            ),
            target_dir,
        )
    };
    c.current_dir(root())
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("NRZANALYTICS_EMBED_WEB", public);
    run(&mut c, &format!("Server-Build {}", t.name()))?;
    let bin = target_dir.join(t.triple()).join("release/nrzanalytics");
    if !bin.is_file() {
        return Err(format!("Programm {} fehlt nach dem Build", bin.display()).into());
    }
    Ok(bin)
}

/// Mit `file` pruefen, dass Linux-Programme statische ELF-Dateien sind.
fn check_binary(t: Target, bin: &Path) -> Result<()> {
    let Some(_) = util::which("file") else {
        println!("    (file nicht vorhanden – Pruefung uebersprungen)");
        return Ok(());
    };
    let desc = util::output(util::cmd("file", &["-b"]).arg(bin), "file")?;
    println!("    {}: {desc}", t.name());
    match t {
        Target::LinuxAmd64 | Target::LinuxArm64 => {
            let arch = if t == Target::LinuxAmd64 {
                "x86-64"
            } else {
                "aarch64"
            };
            if !desc.contains("ELF") || !desc.contains(arch) {
                return Err(format!("{} ist kein ELF-Programm fuer {arch}: {desc}", bin.display()).into());
            }
            if !(desc.contains("statically linked") || desc.contains("static-pie")) {
                return Err(format!("{} ist nicht statisch gelinkt: {desc}", bin.display()).into());
            }
        }
        Target::MacosArm64 => {
            if !desc.contains("Mach-O") || !desc.contains("arm64") {
                return Err(format!("{} ist kein Mach-O-Programm fuer arm64: {desc}", bin.display()).into());
            }
        }
    }
    Ok(())
}

/// `nrzanalytics-<version>-<ziel>.tar.gz` (+ fuer Linux `nrzanalytics-<arch>.gz` fuer Umbrel).
fn package(t: Target, bin: &Path, out: &Path, version: &str) -> Result<()> {
    let root = root();
    let name = format!("nrzanalytics-{version}-{}", t.name());
    let archive = out.join(format!("{name}.tar.gz"));
    let file = fs::File::create(&archive).map_err(|e| format!("{} schreiben: {e}", archive.display()))?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::best());
    let mut tar = tar::Builder::new(enc);
    tar.mode(tar::HeaderMode::Deterministic);
    let mut add = |src: &Path, dst: &str, mode: u32| -> Result<()> {
        let data = fs::read(src).map_err(|e| format!("{} lesen: {e}", src.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, format!("{name}/{dst}"), data.as_slice())?;
        Ok(())
    };
    add(bin, "nrzanalytics", 0o755)?;
    for doc in ["README.md", "LICENSE", "CHANGELOG.md"] {
        if root.join(doc).is_file() {
            add(&root.join(doc), doc, 0o644)?;
        }
    }
    tar.into_inner()?.finish()?;
    if let Some(arch) = t.umbrel_arch() {
        util::gzip(bin, &out.join(format!("nrzanalytics-{arch}.gz")))?;
    }
    Ok(())
}
