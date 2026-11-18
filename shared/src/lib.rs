//! nrzAnalytics – gemeinsame Typen.
//!
//! Wird vom Server, von der Weboberflaeche (Dioxus/WASM) und vom Rust-Client benutzt:
//!
//! - [`query`]: das Abfragemodell (kompatibel mit der Stats-API v2 von Plausible) und das Ergebnis.
//! - [`api`]: Anfragen und Antworten der JSON-API der Oberflaeche (`/api/...`) und die Routen.
//! - [`labels`]: deutsche Beschriftungen fuer Kennzahlen, Dimensionen und Zeitraeume.
//! - [`geo`]: Laendernamen (deutsch) und Flaggen.

pub mod api;
pub mod geo;
pub mod labels;
pub mod query;

/// Version von nrzAnalytics (alle Crates haben dieselbe).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Produktname.
pub const PRODUCT: &str = "nrzAnalytics";
