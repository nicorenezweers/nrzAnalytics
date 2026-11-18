//! Weboberflaeche: Tailwind-Eingabe, dioxus-cli finden, bauen, Entwicklungsserver.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    theme,
    util::{self, Result, root, run},
};

const APP: &str = "nrzanalytics_web";

/// `web/tailwind.css` aus `web/theme.rs` schreiben (+ Platzhalter `web/assets/tailwind.css`).
pub fn css() -> Result<()> {
    let web = root().join("web");
    let input = theme::tailwind_input(&["./src/**/*.rs"]);
    let target = web.join("tailwind.css");
    if fs::read_to_string(&target).ok().as_deref() != Some(input.as_str()) {
        fs::write(&target, &input).map_err(|e| format!("{} schreiben: {e}", target.display()))?;
    }
    let out = web.join("assets").join("tailwind.css");
    if !out.exists() {
        fs::create_dir_all(web.join("assets"))?;
        fs::write(
            &out,
            "/* Platzhalter – dx erzeugt hier das CSS (cargo xtask web). */\n",
        )
        .map_err(|e| format!("{} schreiben: {e}", out.display()))?;
    }
    println!("==> {} ({} Bytes)", target.display(), input.len());
    Ok(())
}

/// dioxus-cli suchen. `dx` im PATH kann auch Denos `dx` sein, darum `--version` pruefen.
pub fn find_dx() -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dx) = std::env::var_os("DX").filter(|v| !v.is_empty()) {
        candidates.push(PathBuf::from(dx));
    }
    candidates.extend(util::which("dx"));
    if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
        candidates.push(PathBuf::from(cargo_home).join("bin/dx"));
    }
    if let Some(home) = util::home() {
        candidates.push(home.join(".cargo/bin/dx"));
    }
    for c in &candidates {
        let version = util::output(&mut util::cmd(c, &["--version"]), "dx --version").unwrap_or_default();
        if version.to_lowercase().contains("dioxus") {
            return Ok(c.clone());
        }
    }
    Err(format!(
        "dioxus-cli (dx) nicht gefunden – installieren mit `cargo binstall dioxus-cli@0.7` \
         oder DX=/pfad/zu/dx setzen (geprueft: {})",
        candidates
            .iter()
            .map(|c| c.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
    .into())
}

/// Oberflaeche mit `dx build` bauen; gibt den public-Ordner zurueck.
pub fn build(release: bool) -> Result<PathBuf> {
    css()?;
    let dx = find_dx()?;
    let web = root().join("web");
    let profile = if release { "release" } else { "debug" };
    println!("==> Oberflaeche ({profile}) mit {}", dx.display());
    let mut c = util::cmd(&dx, &["build", "--platform", "web"]);
    if release {
        c.arg("--release");
    }
    // Feste Ausgabe unter web/target, egal was die Umgebung vorgibt.
    c.current_dir(&web).env_remove("CARGO_TARGET_DIR");
    run(&mut c, "dx build")?;
    let public = web.join("target/dx").join(APP).join(profile).join("web/public");
    ensure_index(&public)?;
    Ok(public)
}

/// Oberflaeche fuer die Auslieferung mit `dx bundle` in einen frischen Ordner bauen
/// (ohne Assets frueherer Builds); gibt `<out>/public` zurueck.
pub fn bundle(build: &Path) -> Result<PathBuf> {
    css()?;
    let dx = find_dx()?;
    let out = build.join("webdist");
    let target = build.join("wasm");
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(target.join("dx").join(APP).join("release/web/public"));
    println!("==> Oberflaeche (release) mit {}", dx.display());
    let mut c = util::cmd(&dx, &["bundle", "--platform", "web", "--release", "--out-dir"]);
    c.arg(&out)
        .current_dir(root().join("web"))
        .env("CARGO_TARGET_DIR", &target);
    run(&mut c, "dx bundle")?;
    let public = out.join("public");
    ensure_index(&public)?;
    Ok(public)
}

fn ensure_index(public: &Path) -> Result<()> {
    if public.join("index.html").is_file() {
        Ok(())
    } else {
        Err(format!("dx hat keine index.html in {} erzeugt", public.display()).into())
    }
}

/// Oberflaeche (debug) bauen und den Server mit `cargo run` starten.
pub fn dev() -> Result<()> {
    let public = build(false)?;
    let root = root();
    let mut c = util::cmd(util::cargo(), &["run", "-p", "nrzanalytics"]);
    c.current_dir(&root);
    let mut set_default = |key: &str, value: &std::ffi::OsStr| {
        if std::env::var_os(key).is_none() {
            c.env(key, value);
        }
    };
    set_default("NRZANALYTICS_WEB_DIR", public.as_os_str());
    set_default("NRZANALYTICS_DATA_DIR", root.join("data").as_os_str());
    set_default("RUST_LOG", "info".as_ref());
    if std::env::var_os("NRZANALYTICS_ADMIN_EMAIL").is_none()
        && std::env::var_os("NRZANALYTICS_ADMIN_PASSWORD").is_none()
    {
        c.env("NRZANALYTICS_ADMIN_EMAIL", "admin")
            .env("NRZANALYTICS_ADMIN_PASSWORD", "admin12345");
        println!(
            "\n  !! Entwicklungs-Admin: Login `admin`, Passwort `admin12345`\n  \
             !! Nur fuer die lokale Entwicklung – NRZANALYTICS_ADMIN_EMAIL/_PASSWORD setzen, um das zu aendern.\n"
        );
    }
    let bind = std::env::var("NRZANALYTICS_BIND").unwrap_or_else(|_| "127.0.0.1:8580".into());
    println!(
        "==> Server: http://{bind}  (Oberflaeche aus {})",
        public.display()
    );
    run(&mut c, "cargo run -p nrzanalytics")
}
