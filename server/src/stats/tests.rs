//! Tests der Auswertung: Ereignisse laufen durch die echte Erfassung (Sitzungen, Schreib-Task) in
//! eine SQLite-Datenbank im Speicher, danach werden Abfragen gegen bekannte Zahlen geprueft.

use super::*;
use crate::{
    config::GeoipMode,
    db::Db,
    geoip::GeoIp,
    ingest::{IncomingEvent, Ingest, RequestInfo},
};
use nrzanalytics_shared::{
    api::{FunnelQuery, GoalKind},
    query::{Comparison, Filter, FilterOp, Query},
};
use std::sync::Arc;

/// 2026-11-17 12:00:00 UTC
const T0: i64 = 1_794_916_800;

const CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";

struct Env {
    db: Db,
    ingest: Arc<Ingest>,
    site: SiteCtx,
}

fn event(name: &str, url: &str, referrer: Option<&str>, width: i64) -> IncomingEvent {
    serde_json::from_value(serde_json::json!({
        "n": name, "u": url, "d": "example.com", "r": referrer, "w": width
    }))
    .unwrap()
}

fn req(ip: &str, ua: &str) -> RequestInfo {
    RequestInfo {
        ip: ip.parse().ok(),
        user_agent: ua.into(),
        country_header: None,
    }
}

async fn setup() -> Env {
    let db = Db::memory();
    db.write(|c| {
        c.execute("INSERT INTO sites (id, domain, timezone, created_at) VALUES (1, 'example.com', 'UTC', 0)", [])?;
        c.execute(
            "INSERT INTO goals (site_id, kind, event_name, display_name, created_at) VALUES (1, 'event', 'Signup', 'Signup', 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO goals (site_id, kind, page_path, display_name, created_at) VALUES (1, 'page', '/blog/**', 'Besuch /blog/**', 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO goals (site_id, kind, page_path, display_name, created_at) VALUES (1, 'page', '/pricing', 'Besuch /pricing', 0)",
            [],
        )?;
        c.execute("INSERT INTO shields (site_id, kind, value, created_at) VALUES (1, 'page', '/admin/**', 0)", [])?;
        Ok(())
    })
    .await
    .unwrap();
    let geo = GeoIp::new(GeoipMode::Off, std::path::Path::new("/tmp"));
    let (ingest, rx) = Ingest::new(db.clone(), geo);
    tokio::spawn(crate::ingest::writer(db.clone(), rx));
    ingest.reload_sites().await.unwrap();
    ingest.rotate_salts().await.unwrap();

    let send = |ev: IncomingEvent, r: RequestInfo, ts: i64| ingest.process_at(&ev, &r, ts).unwrap();
    // Besucher A: Verweis, zwei Seiten, Anmeldung mit Eigenschaft.
    assert_eq!(
        send(
            event(
                "pageview",
                "https://example.com/",
                Some("https://news.example.org/item?id=1"),
                1920
            ),
            req("1.1.1.1", CHROME),
            T0
        ),
        1
    );
    send(
        event("pageview", "https://www.example.com/pricing", None, 1920),
        req("1.1.1.1", CHROME),
        T0 + 60,
    );
    let mut signup = event("Signup", "https://example.com/pricing", None, 1920);
    signup.props = Some(serde_json::json!({"plan": "pro"}));
    send(signup, req("1.1.1.1", CHROME), T0 + 120);
    // Besucher B: Newsletter-Kampagne, ein Blogartikel (Absprung), Smartphone.
    send(
        event(
            "pageview",
            "https://example.com/blog/hallo?utm_source=newsletter&utm_medium=email&utm_campaign=herbst",
            None,
            390,
        ),
        req("2.2.2.2", SAFARI),
        T0 + 300,
    );
    // Besucher C: direkt, liest 12 Sekunden und scrollt 80 %.
    send(
        event("pageview", "https://example.com/", None, 1280),
        req("3.3.3.3", FIREFOX),
        T0 + 600,
    );
    let mut eng = event("engagement", "https://example.com/", None, 1280);
    eng.engagement_time = Some(serde_json::json!(12000));
    eng.scroll_depth = Some(serde_json::json!(80));
    send(eng, req("3.3.3.3", FIREFOX), T0 + 620);
    // Besucher A kommt drei Stunden spaeter direkt wieder (neue Sitzung).
    send(
        event("pageview", "https://example.com/", None, 1920),
        req("1.1.1.1", CHROME),
        T0 + 3 * 3600,
    );
    // Gesperrte Seite, Bot und fremde Domain werden verworfen.
    assert_eq!(
        send(
            event("pageview", "https://example.com/admin/users", None, 1920),
            req("4.4.4.4", CHROME),
            T0 + 700
        ),
        0
    );
    assert_eq!(
        send(
            event("pageview", "https://example.com/", None, 1920),
            req("5.5.5.5", "Googlebot/2.1 (+http://www.google.com/bot.html)"),
            T0 + 700
        ),
        0
    );
    let mut other = event("pageview", "https://other.org/", None, 1920);
    other.domain = "other.org".into();
    assert_eq!(send(other, req("6.6.6.6", CHROME), T0 + 700), 0);
    // Engagement ohne Sitzung zaehlt nicht.
    let mut lonely = event("engagement", "https://example.com/", None, 1280);
    lonely.engagement_time = Some(serde_json::json!(5000));
    assert_eq!(send(lonely, req("7.7.7.7", FIREFOX), T0 + 700), 0);

    ingest.flush().await;
    let goals = db.read(|c| crate::api::load_goals(c, 1)).await.unwrap();
    let site = SiteCtx {
        id: 1,
        domain: "example.com".into(),
        tz: time::tz("UTC"),
        goals,
    };
    Env { db, ingest, site }
}

async fn run(env: &Env, q: Query) -> QueryResult {
    let site = env.site.clone();
    env.db
        .read(move |c| execute(c, &site, &q, T0 + 4 * 3600))
        .await
        .unwrap()
}

fn day() -> DateRange {
    DateRange::named("day")
}

fn num(v: &serde_json::Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

#[tokio::test]
async fn aggregate_metrics() {
    let env = setup().await;
    let q = Query::new(
        &[
            m::VISITORS,
            m::VISITS,
            m::PAGEVIEWS,
            m::EVENTS,
            m::BOUNCE_RATE,
            m::VISIT_DURATION,
            m::VIEWS_PER_VISIT,
        ],
        day(),
    );
    let r = run(&env, q).await;
    let row = &r.results[0];
    assert_eq!(num(&row.metrics[0]), 3.0, "Besucher");
    assert_eq!(num(&row.metrics[1]), 4.0, "Besuche");
    assert_eq!(num(&row.metrics[2]), 5.0, "Seitenaufrufe");
    assert_eq!(num(&row.metrics[3]), 6.0, "Ereignisse ohne Engagement");
    assert_eq!(num(&row.metrics[4]), 75.0, "Absprungrate");
    assert_eq!(num(&row.metrics[5]), 35.0, "Besuchsdauer (120 + 0 + 20 + 0) / 4");
    assert_eq!(num(&row.metrics[6]), 1.25, "Seiten pro Besuch");
    assert_eq!(
        r.meta.date_range,
        Some(("2026-11-17".into(), "2026-11-17".into()))
    );
}

#[tokio::test]
async fn breakdowns() {
    let env = setup().await;
    // Seiten
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::PAGEVIEWS, m::BOUNCE_RATE], day()).dimension(d::EVENT_PAGE),
    )
    .await;
    let pages: Vec<(&str, f64, f64)> = r
        .results
        .iter()
        .map(|row| (row.dimension(0), row.metric(0), row.metric(1)))
        .collect();
    assert_eq!(pages[0], ("/", 2.0, 3.0));
    assert!(pages.contains(&("/pricing", 1.0, 1.0)));
    assert!(pages.contains(&("/blog/hallo", 1.0, 1.0)));
    // Absprungrate der Seite "/" = Besuche, die dort begannen: A1 (nein), C (ja), A2 (ja) → 67 %
    assert_eq!(r.results[0].metric(2), 67.0);
    // Quellen
    let r = run(&env, Query::new(&[m::VISITORS], day()).dimension(d::VISIT_SOURCE)).await;
    let direct = r
        .results
        .iter()
        .find(|row| row.dimension(0) == DIRECT_SOURCE)
        .map(|row| row.metric(0));
    assert_eq!(direct, Some(2.0), "C und A (2. Besuch) direkt");
    assert!(
        r.results.iter().any(
            |row| row.dimension(0).contains("news.example.org") || row.dimension(0) == "news.example.org"
        )
    );
    // UTM: leere Werte fehlen
    let r = run(
        &env,
        Query::new(&[m::VISITORS], day()).dimension(d::VISIT_UTM_CAMPAIGN),
    )
    .await;
    assert_eq!(r.results.len(), 1);
    assert_eq!(r.results[0].dimension(0), "herbst");
    // Einstiegsseiten
    let r = run(
        &env,
        Query::new(&[m::VISITS, m::VISITORS], day()).dimension(d::VISIT_ENTRY_PAGE),
    )
    .await;
    assert_eq!(
        (
            r.results[0].dimension(0),
            r.results[0].metric(0),
            r.results[0].metric(1)
        ),
        ("/", 3.0, 2.0)
    );
    // Ausstiegsrate: /pricing hat 1 Aufruf und 1 Ausstieg → 100 %
    let r = run(
        &env,
        Query::new(&[m::VISITS, m::EXIT_RATE], day()).dimension(d::VISIT_EXIT_PAGE),
    )
    .await;
    let pricing = r
        .results
        .iter()
        .find(|row| row.dimension(0) == "/pricing")
        .unwrap();
    assert_eq!(pricing.metric(1), 100.0);
    // Geraete aus der Fensterbreite
    let r = run(&env, Query::new(&[m::VISITORS], day()).dimension(d::VISIT_DEVICE)).await;
    let devices: Vec<&str> = r.results.iter().map(|row| row.dimension(0)).collect();
    assert!(devices.contains(&"Desktop") && devices.contains(&"Mobile") && devices.contains(&"Laptop"));
    // Seitenaufteilung und Gesamtzahl
    let r = run(
        &env,
        Query::new(&[m::VISITORS], day())
            .dimension(d::EVENT_PAGE)
            .limit(1, 1)
            .total_rows(),
    )
    .await;
    assert_eq!(r.results.len(), 1);
    assert_eq!(r.meta.total_rows, Some(3));
    // Anteil
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::PERCENTAGE], day()).dimension(d::EVENT_PAGE),
    )
    .await;
    assert_eq!(r.results[0].metric(1), 66.7);
}

#[tokio::test]
async fn filters_and_goals() {
    let env = setup().await;
    // Seitenfilter: Besuche, die /pricing enthielten
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::VISITS, m::BOUNCE_RATE], day())
            .filter(Filter::is(d::EVENT_PAGE, "/pricing")),
    )
    .await;
    assert_eq!(
        (
            r.results[0].metric(0),
            r.results[0].metric(1),
            r.results[0].metric(2)
        ),
        (1.0, 1.0, 0.0)
    );
    // Direkt-Filter
    let r = run(
        &env,
        Query::new(&[m::VISITORS], day()).filter(Filter::is(d::VISIT_SOURCE, DIRECT_SOURCE)),
    )
    .await;
    assert_eq!(r.results[0].metric(0), 2.0);
    // Enthaelt (ohne Gross-/Kleinschreibung)
    let f = Filter::Condition {
        op: FilterOp::Contains,
        dimension: d::EVENT_PAGE.into(),
        clauses: vec!["BLOG".into()],
        case_sensitive: false,
    };
    let r = run(&env, Query::new(&[m::VISITORS], day()).filter(f)).await;
    assert_eq!(r.results[0].metric(0), 1.0);
    // ist nicht (Sitzungen ohne /pricing): B, C, A2
    let f = Filter::new(FilterOp::IsNot, d::EVENT_PAGE, &["/pricing"]);
    let r = run(&env, Query::new(&[m::VISITS], day()).filter(f)).await;
    assert_eq!(r.results[0].metric(0), 3.0);
    // Ziel-Filter mit Konversionsrate
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::EVENTS, m::CONVERSION_RATE], day())
            .filter(Filter::is(d::EVENT_GOAL, "Signup")),
    )
    .await;
    assert_eq!((r.results[0].metric(0), r.results[0].metric(1)), (1.0, 1.0));
    assert_eq!(r.results[0].metric(2), 33.33);
    // Ziele als Dimension
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::CONVERSION_RATE], day()).dimension(d::EVENT_GOAL),
    )
    .await;
    let names: Vec<&str> = r.results.iter().map(|row| row.dimension(0)).collect();
    assert!(
        names.contains(&"Signup") && names.contains(&"Besuch /blog/**") && names.contains(&"Besuch /pricing")
    );
    // Eigenschaften mit Ziel-Filter
    let r = run(
        &env,
        Query::new(&[m::VISITORS], day())
            .dimension("event:props:plan")
            .filter(Filter::is(d::EVENT_GOAL, "Signup")),
    )
    .await;
    assert_eq!((r.results[0].dimension(0), r.results[0].metric(0)), ("pro", 1.0));
    // Unbekannte Kennzahl / Konversionsrate ohne Ziel → Fehler
    let site = env.site.clone();
    let err = env
        .db
        .read(move |c| execute(c, &site, &Query::new(&[m::CONVERSION_RATE], day()), T0))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid");
}

#[tokio::test]
async fn timeseries_and_comparison() {
    let env = setup().await;
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::PAGEVIEWS], day())
            .dimension(d::TIME)
            .zero_fill()
            .compare(Some(Comparison::previous_period())),
    )
    .await;
    assert_eq!(r.meta.interval.as_deref(), Some("hour"));
    assert_eq!(r.results.len(), 24);
    assert_eq!(r.results[12].dimension(0), "2026-11-17 12:00:00");
    assert_eq!(r.results[12].metric(0), 3.0);
    assert_eq!(r.results[12].metric(1), 4.0);
    assert_eq!(r.results[15].metric(0), 1.0);
    // Zukunft (nach T0 + 4 h) ist leer statt 0
    assert!(r.results[20].metrics[0].is_null());
    let cmp = r.results[12].comparison.as_ref().unwrap();
    assert_eq!(cmp.dimensions[0], "2026-11-16 12:00:00");
    assert_eq!(cmp.metric(0), 0.0);
    assert!(cmp.change(0).is_none(), "Vergleichswert 0 → keine Prozentangabe");
    // Aggregat mit Vergleich
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::BOUNCE_RATE], day()).compare(Some(Comparison::previous_period())),
    )
    .await;
    let cmp = r.results[0].comparison.as_ref().unwrap();
    assert_eq!(cmp.change(1), Some(75.0), "Raten in Prozentpunkten");
    assert_eq!(
        r.meta.comparison_date_range,
        Some(("2026-11-16".into(), "2026-11-16".into()))
    );
}

#[tokio::test]
async fn engagement_and_funnel() {
    let env = setup().await;
    let r = run(
        &env,
        Query::new(&[m::VISITORS, m::TIME_ON_PAGE, m::SCROLL_DEPTH], day()).dimension(d::EVENT_PAGE),
    )
    .await;
    let home = r.results.iter().find(|row| row.dimension(0) == "/").unwrap();
    assert_eq!(home.metric(1), 12.0);
    assert_eq!(home.metric(2), 80.0);
    // Trichter: /pricing → Signup
    let goals = env.site.goals.clone();
    let funnel = nrzanalytics_shared::api::Funnel {
        id: 1,
        name: "Kauf".into(),
        steps: vec![
            goals
                .iter()
                .find(|g| g.kind == GoalKind::Page && g.page_path.as_deref() == Some("/pricing"))
                .cloned()
                .unwrap(),
            goals
                .iter()
                .find(|g| g.display_name == "Signup")
                .cloned()
                .unwrap(),
        ],
    };
    let site = env.site.clone();
    let q = FunnelQuery {
        funnel_id: 1,
        date_range: day(),
        date: None,
        filters: vec![],
    };
    let res = env
        .db
        .read(move |c| funnel::compute(c, &site, &funnel, &q, T0 + 4 * 3600))
        .await
        .unwrap();
    assert_eq!(res.all_visitors, 3);
    assert_eq!(res.entering_visitors, 1);
    assert_eq!(res.steps[1].visitors, 1);
    assert_eq!(res.conversion_rate, 100.0);
}

#[tokio::test]
async fn suggestions_and_realtime() {
    let env = setup().await;
    let site = env.site.clone();
    let q = nrzanalytics_shared::api::SuggestionQuery {
        dimension: d::EVENT_PAGE.into(),
        q: "pri".into(),
        date_range: day(),
        date: None,
        filters: vec![],
    };
    let list = env
        .db
        .read(move |c| suggest::suggest(c, &site, &q, T0 + 3600))
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].value, "/pricing");
    let n = env
        .db
        .read(|c| current_visitors(c, 1, T0 + 3 * 3600 + 60))
        .await
        .unwrap();
    assert_eq!(n, 1);
    // Export laeuft und liefert ein ZIP
    let site = env.site.clone();
    let zip = env
        .db
        .read(move |c| {
            export::export_zip(
                c,
                &site,
                &nrzanalytics_shared::api::ExportQuery {
                    date_range: day(),
                    ..Default::default()
                },
                T0,
            )
        })
        .await
        .unwrap();
    assert_eq!(&zip[..4], &[0x50, 0x4b, 0x03, 0x04]);
    drop(env.ingest);
}
