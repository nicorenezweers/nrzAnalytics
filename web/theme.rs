// nrzAnalytics – Thema der Oberflaeche (nrzUI, dunkles nrz-Thema als Standard).
//
// Aus diesen Rust-Daten entsteht die Eingabe fuer Tailwind CSS v4 (`web/tailwind.css`, nicht im
// Repository). `web/build.rs` und `cargo xtask css` schreiben sie; `dx` uebersetzt sie dann nach
// `web/assets/tailwind.css`. Farben wie in nrzUI (`tailwind.css`, Block `.dark`).
//
// Diese Datei wird per `include!` (build.rs) bzw. `#[path]` (xtask) eingebunden und darf deshalb
// keine Abhaengigkeiten haben.

/// Farben: (Name, hell, dunkel). Jede Farbe wird zu `--name` und `--color-name` (Tailwind).
pub const COLORS: &[(&str, &str, &str)] = &[
    ("background", "oklch(1 0 0)", "oklch(0.18 0 0)"),
    ("foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
    ("card", "oklch(1 0 0)", "oklch(0.205 0 0)"),
    ("card-foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
    ("popover", "oklch(0.985 0 0)", "oklch(0.235 0 0)"),
    ("popover-foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
    ("primary", "oklch(0.205 0 0)", "oklch(0.922 0 0)"),
    ("primary-foreground", "oklch(0.985 0 0)", "oklch(0.205 0 0)"),
    ("secondary", "oklch(0.92 0 0)", "oklch(0.269 0 0)"),
    ("secondary-foreground", "oklch(0.205 0 0)", "oklch(0.985 0 0)"),
    ("muted", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
    ("muted-foreground", "oklch(0.556 0 0)", "oklch(0.708 0 0)"),
    ("accent", "oklch(0.97 0 0)", "oklch(0.3 0 0)"),
    ("accent-foreground", "oklch(0.205 0 0)", "oklch(0.985 0 0)"),
    ("destructive", "oklch(0.577 0.245 27.325)", "oklch(0.704 0.191 22.216)"),
    ("destructive-light", "oklch(0.95 0.08 27.325)", "oklch(0.25 0.08 22.216)"),
    ("destructive-foreground", "oklch(0.985 0 0)", "oklch(0.985 0 0)"),
    ("border", "oklch(0.922 0 0)", "oklch(1 0 0 / 10%)"),
    ("input", "oklch(0.922 0 0)", "oklch(1 0 0 / 15%)"),
    ("ring", "oklch(0.708 0 0)", "oklch(0.556 0 0)"),
    // Diagramme: Hauptlinie, Vergleich, Gitter, Balken in Listen.
    ("chart-1", "oklch(0.623 0.214 259.815)", "oklch(0.809 0.105 251.813)"),
    ("chart-2", "oklch(0.546 0.245 262.881)", "oklch(0.623 0.214 259.815)"),
    ("chart-3", "oklch(0.488 0.243 264.376)", "oklch(0.546 0.245 262.881)"),
    ("chart-4", "oklch(0.7 0.15 162)", "oklch(0.72 0.14 162)"),
    ("chart-5", "oklch(0.75 0.15 70)", "oklch(0.78 0.14 70)"),
    ("chart-muted", "oklch(0.708 0 0)", "oklch(0.556 0 0)"),
    ("chart-grid", "oklch(0.92 0 0)", "oklch(0.26 0 0)"),
    ("bar", "oklch(0.95 0.03 255)", "oklch(0.3 0.04 255)"),
    ("bar-goal", "oklch(0.94 0.05 160)", "oklch(0.3 0.05 160)"),
    ("sidenav", "oklch(0.985 0 0)", "oklch(0.2 0 0)"),
    ("sidenav-foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
    ("sidenav-accent", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
    ("sidenav-border", "oklch(0.922 0 0)", "oklch(1 0 0 / 8%)"),
    ("selection", "oklch(0.65 0.16 55)", "oklch(0.55 0.18 55)"),
    ("selection-foreground", "oklch(1 0 0)", "oklch(0.205 0 0)"),
    ("success", "oklch(0.65 0.16 145)", "oklch(0.65 0.16 145)"),
    ("success-foreground", "oklch(0.985 0 0)", "oklch(0.205 0 0)"),
    ("warning", "oklch(0.65 0.16 55)", "oklch(0.72 0.15 70)"),
    ("warning-foreground", "oklch(0.985 0 0)", "oklch(0.205 0 0)"),
    ("info", "oklch(0.55 0.18 250)", "oklch(0.68 0.13 250)"),
    ("info-foreground", "oklch(0.985 0 0)", "oklch(0.205 0 0)"),
    ("success-light", "oklch(0.95 0.05 145)", "oklch(0.28 0.06 145)"),
    ("success-dark", "oklch(0.4 0.12 145)", "oklch(0.82 0.13 145)"),
    ("warning-light", "oklch(0.96 0.05 70)", "oklch(0.3 0.06 70)"),
    ("warning-dark", "oklch(0.45 0.12 55)", "oklch(0.85 0.12 70)"),
    ("info-light", "oklch(0.95 0.04 250)", "oklch(0.28 0.06 250)"),
    ("info-dark", "oklch(0.42 0.15 250)", "oklch(0.82 0.1 250)"),
    // Veraenderungen im Dashboard: gut (gruen) / schlecht (rot).
    ("positive", "oklch(0.55 0.15 150)", "oklch(0.75 0.15 150)"),
    ("negative", "oklch(0.58 0.2 27)", "oklch(0.7 0.17 22)"),
    ("live", "oklch(0.7 0.17 150)", "oklch(0.75 0.17 150)"),
    ("scrollbar-thumb", "oklch(0.85 0 0)", "oklch(0.35 0 0)"),
];

/// Weitere Variablen (ohne Tailwind-Farbe): (Name, Wert).
pub const VARIABLES: &[(&str, &str)] = &[
    ("radius", "0.625rem"),
    ("font-sans", "\"Inter\", ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", Roboto, sans-serif"),
    ("font-mono", "ui-monospace, \"SF Mono\", \"JetBrains Mono\", Menlo, Consolas, monospace"),
];

/// Basis-Stile (Tailwind `@layer base` und eigene Utilities).
const BASE: &str = r#"
@layer base {
  * { @apply border-border outline-ring/50; }
  ::selection { @apply bg-selection text-selection-foreground; }
  html { @apply bg-background; }
  body {
    @apply min-h-dvh bg-background text-foreground font-sans antialiased;
    font-synthesis-weight: none;
    text-rendering: optimizeLegibility;
  }
  button:not(:disabled), [role="button"]:not(:disabled) { cursor: pointer; }
  * { scrollbar-width: thin; scrollbar-color: var(--scrollbar-thumb) transparent; }
  ::-webkit-scrollbar { width: 6px; height: 6px; }
  ::-webkit-scrollbar-track { background: transparent; }
  ::-webkit-scrollbar-thumb { background: var(--scrollbar-thumb); border-radius: 6px; }
  input[type="date"]::-webkit-calendar-picker-indicator { filter: var(--date-picker-filter, none); }
  .dark input[type="date"] { color-scheme: dark; }
}

@utility container {
  @apply mx-auto w-full max-w-6xl px-4 lg:px-6;
}

@utility no-scrollbar {
  -ms-overflow-style: none;
  scrollbar-width: none;
  &::-webkit-scrollbar { display: none; }
}

@keyframes nrz-pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: .45; transform: scale(.85); }
}

@utility animate-live {
  animation: nrz-pulse 2s ease-in-out infinite;
}
"#;

/// Komplette Eingabe fuer Tailwind CSS v4. `sources` sind Glob-Muster relativ zur Datei.
pub fn tailwind_input(sources: &[&str]) -> String {
    let mut css = String::from(
        "/* Erzeugt aus web/theme.rs (nrzAnalytics) – nicht von Hand bearbeiten. */\n@import \"tailwindcss\";\n",
    );
    for s in sources {
        css.push_str(&format!("@source \"{s}\";\n"));
    }
    css.push_str("\n@custom-variant dark (&:is(.dark *));\n\n:root {\n");
    for (name, value) in VARIABLES {
        css.push_str(&format!("  --{name}: {value};\n"));
    }
    for (name, light, _) in COLORS {
        css.push_str(&format!("  --{name}: {light};\n"));
    }
    css.push_str("}\n\n.dark {\n  color-scheme: dark;\n");
    for (name, _, dark) in COLORS {
        css.push_str(&format!("  --{name}: {dark};\n"));
    }
    css.push_str("}\n\n@theme inline {\n");
    css.push_str("  --font-sans: var(--font-sans);\n  --font-mono: var(--font-mono);\n");
    css.push_str("  --radius-sm: calc(var(--radius) - 4px);\n  --radius-md: calc(var(--radius) - 2px);\n");
    css.push_str("  --radius-lg: var(--radius);\n  --radius-xl: calc(var(--radius) + 4px);\n");
    for (name, _, _) in COLORS {
        css.push_str(&format!("  --color-{name}: var(--{name});\n"));
    }
    css.push_str("}\n");
    css.push_str(BASE);
    css
}
