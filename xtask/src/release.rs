//! `release`: Tag `v<version>` pushen und auf nrzGit ein Release mit den Dateien aus
//! `target/dist/<version>/` anlegen.

use std::{fs, path::Path, time::Duration};

use serde_json::{Value, json};

use crate::{
    dist::{self, Target},
    util::{self, Result, git, output, root, run},
};

pub fn release(args: &[String]) -> Result<()> {
    let mut version = None;
    let (mut draft, mut dry_run) = (false, false);
    for a in args {
        match a.as_str() {
            "--draft" => draft = true,
            "--dry-run" => dry_run = true,
            s if s.starts_with("--") => return Err(format!("release: unbekannte Option '{s}'").into()),
            s if version.is_none() => version = Some(s.to_string()),
            s => return Err(format!("release: unerwartetes Argument '{s}'").into()),
        }
    }
    let version = version.ok_or("Aufruf: cargo xtask release <version> [--draft] [--dry-run]")?;
    util::check_version(&version)?;
    let ws = util::workspace_version()?;
    if ws != version {
        return Err(format!("Version {version} passt nicht zur Workspace-Version {ws} (Cargo.toml)").into());
    }
    let tag = format!("v{version}");

    let remote = output(
        &mut git(&["remote", "get-url", "origin"]),
        "git remote get-url origin",
    )?;
    let (remote_base, owner, repo) = parse_remote(&remote)?;
    let base = match std::env::var("NRZGIT_URL").ok().filter(|s| !s.is_empty()) {
        Some(url) => url.trim_end_matches('/').to_string(),
        None => remote_base.ok_or_else(|| {
            format!("nrzGit-URL laesst sich aus dem Remote {remote} nicht ableiten – NRZGIT_URL setzen")
        })?,
    };
    let token = std::env::var("NRZGIT_TOKEN").ok().filter(|s| !s.is_empty());
    if token.is_none() && !dry_run {
        return Err(
            "NRZGIT_TOKEN fehlt (Zugangstoken von nrzGit mit Schreibrecht auf das Repository)".into(),
        );
    }

    let title = format!("nrzAnalytics {version}");
    let body = changelog_section(&version).unwrap_or_else(|| {
        format!("{title}\n\nProgramme fuer Linux (amd64, arm64, statisch) und macOS (arm64).")
    });

    let dir = root().join("target/dist").join(&version);
    let mut assets = list_assets(&dir);
    if assets.is_empty() && !dry_run {
        println!("==> {} fehlt – baue zuerst (dist)", dir.display());
        dist::dist(&Target::ALL)?;
        assets = list_assets(&dir);
    }

    println!("==> Release {title} ({owner}/{repo} auf {base})");
    println!(
        "    Tag:      {tag}{}",
        if util::local_tag_exists(&tag) {
            " (vorhanden)"
        } else {
            " (wird angelegt)"
        }
    );
    println!("    Entwurf:  {}", if draft { "ja" } else { "nein" });
    println!(
        "    Anhaenge: {}",
        if assets.is_empty() {
            "(noch keine – dist laeuft beim echten Release)".into()
        } else {
            assets.join(", ")
        }
    );
    println!(
        "    Text:\n{}",
        body.lines()
            .map(|l| format!("      {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if dry_run {
        println!("==> --dry-run: kein Tag, kein Release angelegt");
        return Ok(());
    }
    let token = token.unwrap_or_default();

    // Tag anlegen (falls noetig) und pushen, damit nrzGit ihn kennt.
    if !util::local_tag_exists(&tag) {
        util::ensure_clean()?;
        run(&mut git(&["tag", "-a", &tag, "-m", &title]), "git tag")?;
    }
    run(
        &mut git(&["push", "-q", "origin", &tag]),
        &format!("git push origin {tag}"),
    )?;
    let commit = output(&mut git(&["rev-list", "-n", "1", &tag]), "git rev-list")?;

    let api = format!("{base}/api/v1/repos/{owner}/{repo}/releases");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build();
    let auth = format!("Bearer {token}");

    // Als Entwurf anlegen, Anhaenge hochladen, dann veroeffentlichen (ausser --draft).
    let created = call(
        agent.post(&api).set("Authorization", &auth).send_json(json!({
            "tagName": tag,
            "target": commit,
            "title": title,
            "body": body,
            "isDraft": true,
            "isPrerelease": version.contains('-'),
            "tagMessage": title,
        })),
        "Release anlegen",
    )?;
    let id = match &created["id"] {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return Err(format!("Antwort ohne id: {created}").into()),
    };
    println!("==> Release {id} angelegt");

    for name in &assets {
        let path = dir.join(name);
        println!("    lade {name} hoch ({})", util::size(&path));
        let (content_type, body) = multipart(&path, name)?;
        call(
            agent
                .post(&format!("{api}/{id}/assets"))
                .set("Authorization", &auth)
                .set("Content-Type", &content_type)
                .send_bytes(&body),
            &format!("Anhang {name} hochladen"),
        )?;
    }

    if !draft {
        let published = agent
            .post(&format!("{api}/{id}/publish"))
            .set("Authorization", &auth)
            .call();
        match published {
            Ok(_) => {}
            Err(ureq::Error::Status(404 | 405, _)) => {
                // Aeltere nrzGit-Versionen: Entwurf per PATCH aufheben.
                call(
                    agent
                        .request("PATCH", &format!("{api}/{id}"))
                        .set("Authorization", &auth)
                        .send_json(json!({ "isDraft": false })),
                    "Release veroeffentlichen",
                )?;
            }
            Err(e) => return Err(describe(e, "Release veroeffentlichen").into()),
        }
    }
    println!(
        "==> {} {base}/{owner}/{repo}/releases/tag/{tag}",
        if draft {
            "Entwurf angelegt:"
        } else {
            "Veroeffentlicht:"
        }
    );
    Ok(())
}

fn call(res: std::result::Result<ureq::Response, ureq::Error>, what: &str) -> Result<Value> {
    match res {
        Ok(r) => Ok(r.into_json::<Value>().unwrap_or(Value::Null)),
        Err(e) => Err(describe(e, what).into()),
    }
}

fn describe(e: ureq::Error, what: &str) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let text = r.into_string().unwrap_or_default();
            let hint = match code {
                401 | 403 => " – NRZGIT_TOKEN pruefen (Schreibrecht?)",
                409 => " – gibt es das Release schon?",
                _ => "",
            };
            format!("{what}: HTTP {code}{hint}\n{}", text.trim())
        }
        other => format!("{what}: {other}"),
    }
}

/// Dateien im dist-Ordner (sortiert, SHA256SUMS zuletzt).
fn list_assets(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|n| (n == "SHA256SUMS", n.clone()));
    files
}

/// Abschnitt `## [<version>]` aus CHANGELOG.md (ohne die Ueberschrift).
fn changelog_section(version: &str) -> Option<String> {
    let text = fs::read_to_string(root().join("CHANGELOG.md")).ok()?;
    let head = format!("## [{version}]");
    let mut lines = text.lines().skip_while(|l| !l.starts_with(&head));
    lines.next()?;
    let section: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    let section = section.join("\n").trim().to_string();
    (!section.is_empty()).then_some(section)
}

/// `http(s)://host[:port]/owner/repo(.git)` → (Basis-URL, owner, repo); bei SSH ohne Basis-URL.
fn parse_remote(remote: &str) -> Result<(Option<String>, String, String)> {
    let remote = remote.trim().trim_end_matches('/');
    let (base, path) = if let Some((scheme, rest)) = remote.split_once("://") {
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| format!("Remote {remote} ohne Pfad"))?;
        let host = host.rsplit('@').next().unwrap_or(host);
        let base = matches!(scheme, "http" | "https").then(|| format!("{scheme}://{host}"));
        (base, path.to_string())
    } else if let Some((_, path)) = remote.split_once(':') {
        (None, path.to_string())
    } else {
        return Err(format!("Remote {remote} nicht erkannt").into());
    };
    let path = path.trim_end_matches(".git");
    let mut parts = path.rsplitn(2, '/');
    let repo = parts.next().unwrap_or_default().to_string();
    let owner = parts
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    if owner.is_empty() || repo.is_empty() {
        return Err(format!("owner/repo aus Remote {remote} nicht ableitbar").into());
    }
    Ok((base, owner, repo))
}

/// multipart/form-data mit einem Feld `file` (Dateiname = Anhangname).
fn multipart(path: &Path, name: &str) -> Result<(String, Vec<u8>)> {
    let data = fs::read(path).map_err(|e| format!("{} lesen: {e}", path.display()))?;
    let boundary = format!(
        "----nrzanalytics-{}",
        util::sha256_hex(path)?.get(..24).unwrap_or("x")
    );
    let mime = if name.ends_with(".gz") {
        "application/gzip"
    } else {
        "text/plain"
    };
    let mut body = Vec::with_capacity(data.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
             Content-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok((format!("multipart/form-data; boundary={boundary}"), body))
}

#[cfg(test)]
mod tests {
    use super::parse_remote;

    #[test]
    fn remotes() {
        let (b, o, r) = parse_remote("http://100.78.107.162:8390/nicorenezweers/nrzAnalytics.git").unwrap();
        assert_eq!(b.as_deref(), Some("http://100.78.107.162:8390"));
        assert_eq!((o.as_str(), r.as_str()), ("nicorenezweers", "nrzAnalytics"));
        let (b, o, r) = parse_remote("https://user:pw@git.example.org/a/b").unwrap();
        assert_eq!(b.as_deref(), Some("https://git.example.org"));
        assert_eq!((o.as_str(), r.as_str()), ("a", "b"));
        let (b, o, r) = parse_remote("git@host:nicorenezweers/nrzAnalytics.git").unwrap();
        assert_eq!(
            (b, o.as_str(), r.as_str()),
            (None, "nicorenezweers", "nrzAnalytics")
        );
    }
}
