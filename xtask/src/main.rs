//! `cargo xtask` – Entwicklungs-, Build- und Release-Werkzeuge fuer nrzAnalytics, komplett in Rust
//! (statt Shell-Skripten, Dockerfile oder Justfile). Uebersicht: `cargo xtask help`.

// Gehoert zur Weboberflaeche (eigener Workspace) – hier nicht formatieren.
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../web/theme.rs"]
mod theme;

mod dist;
mod icon;
mod release;
mod umbrel;
mod util;
mod web;

use std::process::ExitCode;

use util::{Result, root, run, workspace_version};

const HELP: &str = "\
nrzAnalytics – Werkzeuge (cargo xtask <befehl>)

  help                              Diese Uebersicht
  css                               web/tailwind.css aus web/theme.rs schreiben
  web [--release]                   Oberflaeche (Dioxus/WASM) mit dx bauen, gibt den public-Ordner aus
  dev                               Oberflaeche (debug) bauen und Server starten (cargo run)
  dist [--target <ziel>]            Release-Programme mit eingebetteter Oberflaeche bauen
                                    Ziele: linux-amd64, linux-arm64, macos-arm64, all (Standard)
                                    Ergebnis: target/dist/<version>/ (tar.gz, Umbrel-.gz, SHA256SUMS)
  umbrel-release <version> [\"notizen\"] [--no-push]
                                    Linux-Programme bauen, Zweig `umbrel` + Tag umbrel-<version>
                                    anlegen und pushen (nrzGit traegt die Version im nrzStore ein)
  release <version> [--draft] [--dry-run]
                                    Tag v<version> pushen und Release auf nrzGit anlegen
                                    (Anhaenge aus target/dist/<version>/; NRZGIT_TOKEN, NRZGIT_URL)
  icon                              umbrel/icon.png zeichnen (256x256, tiny-skia)
  check                             fmt, clippy, Tests und wasm-Check der Oberflaeche

Umgebung: DX (Pfad zu dioxus-cli), NRZANALYTICS_BUILD_DIR (Build-Ordner ohne Leerzeichen,
Standard /tmp/nrzanalytics-build), NRZGIT_TOKEN, NRZGIT_URL.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nFehler: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let rest = &args[args.len().min(1)..];
    match cmd {
        "help" | "-h" | "--help" => {
            println!("{HELP}");
            Ok(())
        }
        "css" => {
            no_args(cmd, rest)?;
            web::css()
        }
        "web" => {
            let release = flags(cmd, rest, &["--release"])?.contains(&"--release");
            let public = web::build(release)?;
            println!("{}", public.display());
            Ok(())
        }
        "dev" => {
            no_args(cmd, rest)?;
            web::dev()
        }
        "dist" => {
            let targets = dist::parse_targets(rest)?;
            let dir = dist::dist(&targets)?;
            println!("==> Fertig: {}", dir.display());
            Ok(())
        }
        "umbrel-release" => umbrel::release(rest),
        "release" => release::release(rest),
        "icon" => {
            no_args(cmd, rest)?;
            icon::icon()
        }
        "check" => {
            no_args(cmd, rest)?;
            check()
        }
        other => Err(format!("unbekannter Befehl '{other}' – siehe cargo xtask help").into()),
    }
}

fn no_args(cmd: &str, rest: &[String]) -> Result<()> {
    match rest.first() {
        Some(a) => Err(format!("{cmd}: unerwartetes Argument '{a}'").into()),
        None => Ok(()),
    }
}

/// Nur die erlaubten Schalter zulassen; gibt die gesetzten zurueck.
fn flags<'a>(cmd: &str, rest: &'a [String], allowed: &[&str]) -> Result<Vec<&'a str>> {
    rest.iter()
        .map(|a| {
            if allowed.contains(&a.as_str()) {
                Ok(a.as_str())
            } else {
                Err(format!(
                    "{cmd}: unbekanntes Argument '{a}' (erlaubt: {})",
                    allowed.join(", ")
                )
                .into())
            }
        })
        .collect()
}

fn check() -> Result<()> {
    let root = root();
    println!("==> nrzAnalytics {} pruefen", workspace_version()?);
    let cargo = util::cargo();
    run(
        util::cmd(&cargo, &["fmt", "--all", "--", "--check"]).current_dir(&root),
        "cargo fmt --check",
    )?;
    run(
        util::cmd(
            &cargo,
            &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        )
        .current_dir(&root),
        "cargo clippy",
    )?;
    run(
        util::cmd(&cargo, &["test", "--workspace"]).current_dir(&root),
        "cargo test",
    )?;
    web::css()?;
    run(
        util::cmd(&cargo, &["check", "--target", "wasm32-unknown-unknown"]).current_dir(root.join("web")),
        "cargo check (web, wasm32)",
    )?;
    println!("==> Alles in Ordnung");
    Ok(())
}
