//! JSON-API der Weboberflaeche (`/api/...`).
//!
//! Alle Antworten sind JSON. Fehler kommen als [`ApiError`] mit passendem HTTP-Status.
//! Schreibende Anfragen (POST/PUT/DELETE) brauchen den Header [`CSRF_HEADER`] (`x-nrz-analytics: 1`)
//! und eine passende `Origin`. Angemeldet wird per HttpOnly-Cookie (`nrz_analytics_session`).
//! Zeitstempel sind Unix-Sekunden (UTC).
//!
//! # Routen
//!
//! | Methode | Pfad | Anfrage | Antwort |
//! |---------|------|---------|---------|
//! | GET | `/api/status` | – | [`Status`] |
//! | GET | `/api/timezones` | – | `Vec<`[`TimezoneOption`]`>` |
//! | POST | `/api/auth/login` | [`LoginRequest`] | [`Me`] (401 mit `code = "totp_required"`, wenn der 2FA-Code fehlt) |
//! | POST | `/api/auth/register` | [`RegisterRequest`] | [`Me`] (erstes Konto = Admin) |
//! | POST | `/api/auth/logout` | – | 204 |
//! | PUT | `/api/account` | [`UpdateAccount`] | [`Me`] |
//! | PUT | `/api/account/password` | [`ChangePassword`] | 204 |
//! | POST | `/api/account/2fa/setup` | – | [`TotpSetup`] |
//! | POST | `/api/account/2fa/enable` | [`TotpEnable`] | [`RecoveryCodes`] |
//! | POST | `/api/account/2fa/disable` | [`TotpDisable`] | 204 |
//! | GET | `/api/account/api-keys` | – | `Vec<`[`ApiKey`]`>` |
//! | POST | `/api/account/api-keys` | [`NewApiKey`] | [`CreatedApiKey`] |
//! | DELETE | `/api/account/api-keys/{id}` | – | 204 |
//! | GET | `/api/sites` | – | `Vec<`[`SiteSummary`]`>` |
//! | POST | `/api/sites` | [`NewSite`] | [`Site`] |
//! | GET | `/api/sites/{domain}` | – | [`Site`] (auch oeffentlich bzw. mit [`SHARE_HEADER`]) |
//! | PUT | `/api/sites/{domain}` | [`UpdateSite`] | [`Site`] |
//! | DELETE | `/api/sites/{domain}` | – | 204 |
//! | POST | `/api/sites/{domain}/reset` | – | 204 (alle Statistiken loeschen) |
//! | GET | `/api/sites/{domain}/verify` | – | [`Verification`] |
//! | GET/POST | `/api/sites/{domain}/goals` | – / [`NewGoal`] | `Vec<`[`Goal`]`>` / [`Goal`] |
//! | DELETE | `/api/sites/{domain}/goals/{id}` | – | 204 |
//! | GET/POST | `/api/sites/{domain}/funnels` | – / [`NewFunnel`] | `Vec<`[`Funnel`]`>` / [`Funnel`] |
//! | PUT/DELETE | `/api/sites/{domain}/funnels/{id}` | [`NewFunnel`] / – | [`Funnel`] / 204 |
//! | GET/PUT | `/api/sites/{domain}/props` | – / [`UpdateProps`] | [`PropsSettings`] |
//! | GET/POST | `/api/sites/{domain}/members` | – / [`NewMember`] | `Vec<`[`Member`]`>` / [`Member`] |
//! | PUT/DELETE | `/api/sites/{domain}/members/{user_id}` | [`UpdateMember`] / – | [`Member`] / 204 |
//! | GET/POST | `/api/sites/{domain}/shared-links` | – / [`NewSharedLink`] | `Vec<`[`SharedLink`]`>` / [`SharedLink`] |
//! | DELETE | `/api/sites/{domain}/shared-links/{id}` | – | 204 |
//! | GET/POST | `/api/sites/{domain}/shields` | – / [`NewShield`] | `Vec<`[`Shield`]`>` / [`Shield`] |
//! | DELETE | `/api/sites/{domain}/shields/{id}` | – | 204 |
//! | POST | `/api/stats/{domain}/query` | [`crate::query::Query`] | [`crate::query::QueryResult`] |
//! | GET | `/api/stats/{domain}/realtime` | – | [`Realtime`] |
//! | POST | `/api/stats/{domain}/suggestions` | [`SuggestionQuery`] | `Vec<`[`Suggestion`]`>` |
//! | POST | `/api/stats/{domain}/funnel` | [`FunnelQuery`] | [`FunnelResult`] |
//! | GET | `/api/stats/{domain}/export?q=<ExportQuery als JSON>&share=<slug>` | – | ZIP mit CSV-Dateien |
//! | GET | `/api/share/{slug}` | – | [`ShareInfo`] |
//! | POST | `/api/share/{slug}/auth` | [`SharePassword`] | 204 (setzt ein Cookie fuer diesen Link) |
//! | GET/POST | `/api/admin/users` | – / [`NewUser`] | `Vec<`[`AdminUser`]`>` / [`AdminUser`] |
//! | PUT/DELETE | `/api/admin/users/{id}` | [`UpdateUser`] / – | [`AdminUser`] / 204 |
//! | GET/PUT | `/api/admin/settings` | – / [`UpdateInstanceSettings`] | [`InstanceSettings`] |
//!
//! Statistik-Routen und `GET /api/sites/{domain}` funktionieren ohne Anmeldung, wenn die Website
//! oeffentlich ist oder der Header [`SHARE_HEADER`] einen gueltigen geteilten Link nennt.

use crate::query::{DateRange, Filter};
use serde::{Deserialize, Serialize};

/// Header, den alle schreibenden Anfragen der Oberflaeche mitschicken muessen (Wert `1`).
pub const CSRF_HEADER: &str = "x-nrz-analytics";
/// Header mit dem Kurznamen (`slug`) eines geteilten Links.
pub const SHARE_HEADER: &str = "x-nrz-share";
/// Name des Sitzungs-Cookies.
pub const SESSION_COOKIE: &str = "nrz_analytics_session";

/// Fehlerantwort.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    /// Maschinenlesbar: `unauthorized`, `forbidden`, `not_found`, `invalid`, `conflict`,
    /// `rate_limited`, `totp_required`, `share_password_required`, `internal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Instanz und Anmeldung
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    /// Noch kein Konto: die Oberflaeche zeigt die Einrichtung (erstes Konto = Admin).
    pub needs_setup: bool,
    pub registration_open: bool,
    /// Oeffentliche Adresse fuer Tracking-Snippets, z. B. `https://analytics.example.com`.
    pub public_url: String,
    /// Angemeldetes Konto, falls vorhanden.
    #[serde(default)]
    pub user: Option<Me>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub is_admin: bool,
    pub totp_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimezoneOption {
    /// IANA-Name, z. B. `Europe/Zurich`.
    pub name: String,
    /// Anzeige mit aktuellem Versatz, z. B. `(GMT+01:00) Europe/Zurich`.
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    /// E-Mail oder Benutzername.
    pub email: String,
    pub password: String,
    /// TOTP-Code oder Wiederherstellungscode, wenn 2FA aktiv ist.
    #[serde(default)]
    pub totp: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub password: String,
}

// ---------------------------------------------------------------------------------------------
// Konto
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAccount {
    pub email: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePassword {
    pub current: String,
    pub new: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpSetup {
    /// Geheimnis in Base32 (zum Abtippen).
    pub secret: String,
    /// `otpauth://totp/...` fuer den QR-Code.
    pub otpauth_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpEnable {
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpDisable {
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryCodes {
    /// Einmal-Codes, werden nur jetzt angezeigt.
    pub codes: Vec<String>,
}

/// Berechtigung eines API-Schluessels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApiScope {
    /// Stats-API (v1/v2) und MCP lesen.
    #[default]
    Stats,
    /// Zusaetzlich Websites, Ziele und geteilte Links anlegen/loeschen (Sites-API, MCP schreiben).
    Sites,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: i64,
    pub name: String,
    /// Anfang des Schluessels zur Wiedererkennung, z. B. `nrzan_AbC1`.
    pub prefix: String,
    pub scope: ApiScope,
    pub created_at: i64,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewApiKey {
    pub name: String,
    #[serde(default)]
    pub scope: ApiScope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedApiKey {
    /// Vollstaendiger Schluessel – wird nur einmal angezeigt.
    pub key: String,
    pub api_key: ApiKey,
}

// ---------------------------------------------------------------------------------------------
// Websites
// ---------------------------------------------------------------------------------------------

/// Rolle in einer Website.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Nur Statistiken ansehen.
    Viewer,
    /// Zusaetzlich Ziele, Trichter, Eigenschaften, Schutzregeln und geteilte Links.
    Editor,
    /// Zusaetzlich Mitglieder, Sichtbarkeit, allgemeine Einstellungen, Statistiken zuruecksetzen.
    Admin,
    /// Alles, auch die Website loeschen.
    Owner,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Owner, Role::Admin, Role::Editor, Role::Viewer];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Viewer => "Betrachter",
            Self::Editor => "Bearbeiter",
            Self::Admin => "Admin",
            Self::Owner => "Besitzer",
        }
    }

    pub fn can_edit(self) -> bool {
        self >= Self::Editor
    }

    pub fn can_admin(self) -> bool {
        self >= Self::Admin
    }
}

/// Eintrag der Website-Liste.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteSummary {
    pub domain: String,
    pub timezone: String,
    pub public: bool,
    #[serde(default)]
    pub role: Option<Role>,
    /// Besucher der letzten 24 Stunden.
    pub visitors_24h: u64,
    /// Veraenderung gegenueber den 24 Stunden davor in Prozent.
    #[serde(default)]
    pub change_24h: Option<f64>,
    /// Besucher je Stunde, die letzten 24 Stunden (aelteste zuerst).
    #[serde(default)]
    pub sparkline: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSite {
    pub domain: String,
    pub timezone: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub id: i64,
    pub domain: String,
    pub timezone: String,
    pub public: bool,
    /// Rolle des angemeldeten Kontos; `None` fuer Besucher eines oeffentlichen Dashboards oder
    /// geteilten Links.
    #[serde(default)]
    pub role: Option<Role>,
    pub created_at: i64,
    /// Erster Tag mit Daten (`YYYY-MM-DD`, Zeitzone der Website).
    #[serde(default)]
    pub stats_start: Option<String>,
    #[serde(default)]
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub funnels: Vec<Funnel>,
    /// Bekannte eigene Eigenschaften (fuer die Auswertung).
    #[serde(default)]
    pub props: Vec<String>,
    /// URL des Tracking-Skripts, z. B. `https://analytics.example.com/js/script.js`.
    pub script_url: String,
    /// Fertiges Snippet fuer `<head>`.
    pub snippet: String,
    /// Dashboard laeuft ueber einen geteilten Link (ohne Navigation zu Einstellungen).
    #[serde(default)]
    pub shared: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateSite {
    /// Neue Domain (nach dem Wechsel muss das Snippet angepasst werden).
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub public: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    /// Mindestens ein Ereignis ist angekommen.
    pub received: bool,
    #[serde(default)]
    pub last_event_at: Option<i64>,
    /// Snippet auf der Startseite der Website gefunden (`None` = Seite nicht erreichbar).
    #[serde(default)]
    pub snippet_found: Option<bool>,
    /// Erklaerung fuer Menschen.
    pub message: String,
}

// ---------------------------------------------------------------------------------------------
// Ziele, Trichter, Eigenschaften
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalKind {
    /// Eigenes Ereignis (`plausible('Signup')`).
    Event,
    /// Seitenaufruf eines Pfads, Platzhalter `*` (ohne `/`) und `**` (alles).
    Page,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub id: i64,
    pub kind: GoalKind,
    #[serde(default)]
    pub event_name: Option<String>,
    #[serde(default)]
    pub page_path: Option<String>,
    /// Anzeigename, wird auch als Wert der Dimension `event:goal` benutzt.
    pub display_name: String,
    /// Waehrung bei Umsatz-Zielen (ISO 4217, z. B. `CHF`).
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewGoal {
    pub kind: GoalKind,
    #[serde(default)]
    pub event_name: Option<String>,
    #[serde(default)]
    pub page_path: Option<String>,
    /// Standard: Ereignisname bzw. `Besuch /pfad`.
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Funnel {
    pub id: i64,
    pub name: String,
    /// Schritte in Reihenfolge (2 bis 8 Ziele).
    pub steps: Vec<Goal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewFunnel {
    pub name: String,
    pub goal_ids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FunnelQuery {
    pub funnel_id: i64,
    #[serde(default)]
    pub date_range: DateRange,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FunnelResult {
    pub name: String,
    /// Alle Besucher im Zeitraum.
    pub all_visitors: u64,
    /// Besucher, die Schritt 1 erreicht haben.
    pub entering_visitors: u64,
    pub entering_visitors_percentage: f64,
    pub never_entering_visitors: u64,
    /// Gesamt: letzter Schritt / erster Schritt in Prozent.
    pub conversion_rate: f64,
    pub steps: Vec<FunnelStepResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FunnelStepResult {
    pub label: String,
    pub visitors: u64,
    /// Anteil an den Besuchern von Schritt 1 in Prozent.
    pub conversion_rate: f64,
    /// Anteil am vorherigen Schritt in Prozent.
    pub conversion_rate_step: f64,
    /// Verloren seit dem vorherigen Schritt.
    pub dropoff: u64,
    pub dropoff_percentage: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropsSettings {
    /// Erlaubte Eigenschaften; `None` = alle werden gespeichert.
    #[serde(default)]
    pub allowed: Option<Vec<String>>,
    /// In den letzten 30 Tagen gesehene Eigenschaften.
    pub seen: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProps {
    #[serde(default)]
    pub allowed: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------------------------
// Mitglieder, geteilte Links, Schutzregeln
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user_id: i64,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub created_at: i64,
}

/// Bestehendes Konto (E-Mail oder Benutzername) zur Website hinzufuegen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewMember {
    pub email: String,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateMember {
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedLink {
    pub id: i64,
    pub name: String,
    pub slug: String,
    /// Vollstaendige URL, z. B. `https://analytics.example.com/share/example.com?auth=abc`.
    pub url: String,
    pub has_password: bool,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSharedLink {
    pub name: String,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareInfo {
    pub domain: String,
    pub name: String,
    pub needs_password: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePassword {
    pub password: String,
}

/// Art einer Schutzregel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShieldKind {
    /// IP-Adresse oder Netz (CIDR) blockieren.
    Ip,
    /// Land (ISO-Code) blockieren.
    Country,
    /// Seite (Pfad, Platzhalter `*`/`**`) nicht erfassen.
    Page,
    /// Nur diese Hostnamen erfassen (Platzhalter `*.example.com`). Ohne Eintrag: alle.
    Hostname,
}

impl ShieldKind {
    pub const ALL: [ShieldKind; 4] = [Self::Ip, Self::Country, Self::Page, Self::Hostname];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ip => "ip",
            Self::Country => "country",
            Self::Page => "page",
            Self::Hostname => "hostname",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ip => "IP-Adressen",
            Self::Country => "Länder",
            Self::Page => "Seiten",
            Self::Hostname => "Hostnamen",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shield {
    pub id: i64,
    pub kind: ShieldKind,
    pub value: String,
    #[serde(default)]
    pub description: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewShield {
    pub kind: ShieldKind,
    pub value: String,
    #[serde(default)]
    pub description: String,
}

// ---------------------------------------------------------------------------------------------
// Statistik
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Realtime {
    /// Besucher der letzten 5 Minuten.
    pub visitors: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SuggestionQuery {
    /// Dimension, z. B. `visit:source` oder `event:props:author`; `event:props` liefert die
    /// Namen der Eigenschaften.
    pub dimension: String,
    /// Suchtext (enthaelt, ohne Gross-/Kleinschreibung).
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub date_range: DateRange,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// Wert fuer den Filter (z. B. Laendercode `CH`).
    pub value: String,
    /// Anzeige (z. B. `Schweiz`).
    pub label: String,
}

/// Parameter `q` von `/api/stats/{domain}/export` (als JSON, URL-kodiert).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct ExportQuery {
    #[serde(default)]
    pub date_range: DateRange,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

// ---------------------------------------------------------------------------------------------
// Verwaltung (nur Instanz-Admins)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminUser {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub is_admin: bool,
    pub totp_enabled: bool,
    pub created_at: i64,
    #[serde(default)]
    pub last_login_at: Option<i64>,
    /// Anzahl Websites, in denen das Konto Mitglied ist.
    pub sites: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewUser {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateUser {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_admin: Option<bool>,
    /// Neues Passwort (meldet alle Geraete ab).
    #[serde(default)]
    pub password: Option<String>,
    /// 2FA zuruecksetzen (wenn das Konto den Zugang verloren hat).
    #[serde(default)]
    pub disable_totp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeoipStatus {
    /// Laenderzuordnung aktiv.
    pub enabled: bool,
    /// Art der Datenbank, z. B. `DBIP-Country-Lite`.
    #[serde(default)]
    pub database: Option<String>,
    /// Stand der Datenbank (`YYYY-MM-DD`).
    #[serde(default)]
    pub build_date: Option<String>,
    /// Regionen und Staedte verfuegbar.
    pub city_level: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSettings {
    pub registration_open: bool,
    /// Oeffentliche Adresse fuer Snippets und geteilte Links (leer = Adresse der Anfrage).
    pub public_url: String,
    pub geoip: GeoipStatus,
    /// Anzahl gespeicherter Ereignisse (alle Websites).
    pub total_events: u64,
    /// Groesse der Datenbank in Bytes.
    pub database_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateInstanceSettings {
    #[serde(default)]
    pub registration_open: Option<bool>,
    #[serde(default)]
    pub public_url: Option<String>,
}
