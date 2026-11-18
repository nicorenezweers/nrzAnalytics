//! `umbrel-release`: Linux-Programme bauen, auf den Zweig `umbrel` legen und `umbrel-<version>`
//! taggen. nrzGit traegt die Version dann per `nrzStore.json` selbst im nrzStore ein
//! (RELEASE in hooks/pre-start, version, releaseNotes, Store-Version).

use std::fs;

use crate::{
    dist::{self, Target},
    util::{self, RemoveOnDrop, Result, git, git_in, output, run},
};

const FILES: [&str; 3] = ["nrzanalytics-amd64.gz", "nrzanalytics-arm64.gz", "SHA256SUMS"];

pub fn release(args: &[String]) -> Result<()> {
    let mut positional = Vec::new();
    let mut push = true;
    for a in args {
        match a.as_str() {
            "--no-push" => push = false,
            s if s.starts_with("--") => return Err(format!("umbrel-release: unbekannte Option '{s}'").into()),
            s => positional.push(s),
        }
    }
    let (version, notes) = match positional.as_slice() {
        [v] => (*v, format!("nrzAnalytics {v}")),
        [v, n] => (*v, n.to_string()),
        _ => return Err("Aufruf: cargo xtask umbrel-release <version> [\"notizen\"] [--no-push]".into()),
    };
    util::check_version(version)?;
    let tag = format!("umbrel-{version}");

    util::ensure_clean()?;
    if util::local_tag_exists(&tag) {
        return Err(format!("Tag {tag} gibt es schon (lokal).").into());
    }
    if util::remote_tag_exists(&tag) == Some(true) {
        return Err(format!("Tag {tag} gibt es schon auf origin.").into());
    }
    let ws = util::workspace_version()?;
    if ws != version {
        return Err(format!(
            "Version {version} passt nicht zur Workspace-Version {ws} – erst [workspace.package] version \
             in Cargo.toml (und CHANGELOG.md) anpassen und committen"
        )
        .into());
    }
    let readme = output(
        &mut git(&["show", "HEAD:xtask/umbrel-branch-README.md"]),
        "xtask/umbrel-branch-README.md lesen",
    )?;

    // Programme bauen und nur die Umbrel-Dateien mit eigener SHA256SUMS bereitstellen.
    let dir = dist::dist(&[Target::LinuxAmd64, Target::LinuxArm64])?;
    let staging = util::temp_dir("nrzanalytics-umbrel-files")?;
    let _staging_guard = RemoveOnDrop(staging.clone());
    for f in &FILES[..2] {
        fs::copy(dir.join(f), staging.join(f)).map_err(|e| format!("{f} kopieren: {e}"))?;
    }
    util::write_sha256sums(
        &staging,
        &FILES[..2].iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )?;

    // Zweig `umbrel` in einem eigenen Worktree aktualisieren (nur fertige Dateien).
    println!("==> Zweig umbrel aktualisieren");
    let wt = util::temp_dir("nrzanalytics-umbrel-wt")?;
    let _ = fs::remove_dir(&wt); // git worktree add legt den Ordner selbst an
    let wt_str = wt.to_string_lossy().to_string();
    let _ = util::succeeds(&mut git(&["fetch", "-q", "origin", "umbrel"]));
    let has_remote = util::succeeds(&mut git(&[
        "rev-parse",
        "-q",
        "--verify",
        "refs/remotes/origin/umbrel",
    ]));
    let has_local = util::succeeds(&mut git(&["rev-parse", "-q", "--verify", "refs/heads/umbrel"]));
    let guard = Worktree(wt_str.clone());
    if has_remote {
        run(
            &mut git(&["worktree", "add", "-q", "-B", "umbrel", &wt_str, "origin/umbrel"]),
            "git worktree add",
        )?;
    } else if has_local {
        run(
            &mut git(&["worktree", "add", "-q", &wt_str, "umbrel"]),
            "git worktree add",
        )?;
    } else {
        run(
            &mut git(&["worktree", "add", "-q", "--detach", &wt_str]),
            "git worktree add",
        )?;
        run(
            &mut git_in(&wt, &["checkout", "-q", "--orphan", "umbrel"]),
            "git checkout --orphan umbrel",
        )?;
        run(&mut git_in(&wt, &["rm", "-rqf", "."]), "git rm")?;
    }

    for f in FILES {
        fs::copy(staging.join(f), wt.join(f)).map_err(|e| format!("{f} kopieren: {e}"))?;
    }
    fs::write(wt.join("README.md"), format!("{readme}\n"))?;
    run(&mut git_in(&wt, &["add", "-A"]), "git add")?;
    run(
        &mut git_in(
            &wt,
            &[
                "commit",
                "-q",
                "-m",
                &format!("nrzAnalytics {version}: Umbrel-Paket"),
            ],
        ),
        "git commit (Zweig umbrel)",
    )?;
    run(&mut git_in(&wt, &["tag", "-a", &tag, "-m", &notes]), "git tag")?;
    if push {
        run(
            &mut git_in(&wt, &["push", "-q", "origin", "umbrel"]),
            "git push origin umbrel",
        )?;
        run(
            &mut git_in(&wt, &["push", "-q", "origin", &tag]),
            &format!("git push origin {tag}"),
        )?;
    }
    drop(guard);

    if push {
        println!("==> {tag} veroeffentlicht. nrzGit aktualisiert den nrzStore.");
    } else {
        println!(
            "==> {tag} lokal angelegt (--no-push, nichts gepusht).\n    \
             Spaeter veroeffentlichen: git push origin umbrel {tag}\n    \
             Oder verwerfen:           git tag -d {tag} && git branch -D umbrel"
        );
    }
    Ok(())
}

/// Entfernt den temporaeren Worktree auch bei Fehlern.
struct Worktree(String);

impl Drop for Worktree {
    fn drop(&mut self) {
        let _ = util::succeeds(&mut git(&["worktree", "remove", "--force", &self.0]));
        let _ = fs::remove_dir_all(&self.0);
        let _ = util::succeeds(&mut git(&["worktree", "prune"]));
    }
}
