//! Schreibt die Tailwind-Eingabe `tailwind.css` aus `theme.rs` (nicht im Repository) und legt einen
//! Platzhalter fuer `assets/tailwind.css` an, damit `asset!` auch ohne `dx` (z. B. `cargo check`)
//! uebersetzt. Das echte CSS erzeugt `dx` (Tailwind v4) beim Bauen.

#[allow(dead_code)]
mod theme {
    include!("theme.rs");
}

use std::{fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=theme.rs");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = theme::tailwind_input(&["./src/**/*.rs"]);
    let target = dir.join("tailwind.css");
    if fs::read_to_string(&target).ok().as_deref() != Some(input.as_str()) {
        let _ = fs::write(&target, input);
    }
    let out = dir.join("assets").join("tailwind.css");
    if !out.exists() {
        let _ = fs::create_dir_all(dir.join("assets"));
        let _ = fs::write(out, "/* Platzhalter – dx erzeugt hier das CSS (cargo xtask web). */\n");
    }
}
