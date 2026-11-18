//! Bettet die Weboberflaeche ins Programm ein, wenn `NRZANALYTICS_EMBED_WEB` beim Bauen auf den
//! Ordner der fertigen Oberflaeche zeigt (macht `cargo xtask dist`). Sonst bleibt die Liste leer
//! und der Server liest die Oberflaeche zur Laufzeit aus `NRZANALYTICS_WEB_DIR`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-env-changed=NRZANALYTICS_EMBED_WEB");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default()).join("embedded_web.rs");
    let mut code = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    if let Ok(dir) = env::var("NRZANALYTICS_EMBED_WEB") {
        let root = PathBuf::from(&dir);
        let mut files = Vec::new();
        collect(&root, &root, &mut files);
        files.sort();
        assert!(
            files.iter().any(|(rel, _)| rel == "index.html"),
            "NRZANALYTICS_EMBED_WEB={dir}: index.html fehlt"
        );
        for (rel, abs) in files {
            println!("cargo:rerun-if-changed={}", abs.display());
            code.push_str(&format!(
                "    ({rel:?}, include_bytes!({:?})),\n",
                abs.display().to_string()
            ));
        }
    }
    code.push_str("];\n");
    fs::write(out, code).expect("embedded_web.rs schreiben");
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            let abs = path.canonicalize().unwrap_or(path.clone());
            files.push((rel, abs));
        }
    }
}
