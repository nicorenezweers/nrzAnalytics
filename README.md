# nrzAnalytics – Umbrel-Programme

Dieser Zweig enthaelt nur fertig gebaute Programme fuer die Umbrel-App `nrz-analytics` aus dem
nrzStore. Den Quelltext gibt es auf dem Zweig `main`.

| Datei | Inhalt |
|-------|--------|
| `nrzanalytics-amd64.gz` | Server mit eingebetteter Oberflaeche, Linux x86_64 (statisch, musl), gzip |
| `nrzanalytics-arm64.gz` | dasselbe fuer Linux aarch64 |
| `SHA256SUMS` | Pruefsummen (`sha256sum -c`) |

`hooks/pre-start` der App laedt beim Start die Dateien des Tags `umbrel-<version>` fuer die
Architektur des Geraets, prueft die Pruefsumme und entpackt das Programm nach
`${APP_DATA_DIR}/app/nrzanalytics`.

Gebaut und veroeffentlicht mit `cargo xtask umbrel-release <version> "notizen"` – nicht von Hand
bearbeiten.
