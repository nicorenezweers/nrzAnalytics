//! Tests der HTTP-API: Einrichtung, Anmeldung, CSRF, Websites, Erfassung, Statistik, geteilte
//! Links, API-Schluessel (Stats-API v1/v2) und MCP.

use crate::{
    AppState, Instance, app,
    config::{Config, GeoipMode},
    db::Db,
    geoip::GeoIp,
    ingest::Ingest,
    security::RateLimiter,
};
use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use tower::ServiceExt;

struct TestApp {
    router: Router,
    state: AppState,
}

async fn test_app() -> TestApp {
    let db = Db::memory();
    let geo = GeoIp::new(GeoipMode::Off, std::path::Path::new("/tmp"));
    let (ingest, rx) = Ingest::new(db.clone(), geo.clone());
    tokio::spawn(crate::ingest::writer(db.clone(), rx));
    ingest.rotate_salts().await.unwrap();
    let config = Config {
        bind: "127.0.0.1:8580".parse().unwrap(),
        public_bind: None,
        data_dir: "/tmp".into(),
        database: "/tmp/nrzanalytics-test.db".into(),
        web_dir: None,
        admin_email: None,
        admin_password: None,
        secret: None,
        public_url: Some("http://analytics.test".into()),
        trust_proxy: false,
        secure_cookies: false,
        geoip: GeoipMode::Off,
        retention_days: 0,
        registration: None,
    };
    let state = AppState {
        config: Arc::new(config),
        db,
        ingest,
        geo,
        limiter: Arc::new(RateLimiter::default()),
        secret: Arc::new(b"test-secret".to_vec()),
        instance: Arc::new(RwLock::new(Instance::default())),
    };
    TestApp {
        router: app(state.clone(), None),
        state,
    }
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
    text: String,
}

impl TestApp {
    async fn call(&self, method: Method, uri: &str, body: Option<Value>, headers: &[(&str, &str)]) -> Resp {
        let mut b = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header(header::HOST, "analytics.test");
        if method != Method::GET {
            b = b.header("x-nrz-analytics", "1");
        }
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let mut req = match body {
            Some(v) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 4711))));
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        Resp {
            status,
            headers,
            json: serde_json::from_str(&text).unwrap_or(Value::Null),
            text,
        }
    }
}

fn session_cookie(r: &Resp) -> String {
    let raw = r
        .headers
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    raw.split(';').next().unwrap_or("").to_string()
}

#[tokio::test]
async fn full_flow() {
    let t = test_app().await;

    // Einrichtung: erstes Konto wird Admin.
    let r = t.call(Method::GET, "/api/status", None, &[]).await;
    assert_eq!(r.json["needs_setup"], true);
    let r = t
        .call(
            Method::POST,
            "/api/auth/register",
            Some(json!({"email": "admin", "password": "sehr-geheim-123"})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["is_admin"], true);
    let cookie = session_cookie(&r);
    assert!(cookie.starts_with("nrz_analytics_session="));
    let auth = [("cookie", cookie.as_str())];

    // Zweite Registrierung ist geschlossen.
    let r = t
        .call(
            Method::POST,
            "/api/auth/register",
            Some(json!({"email": "x@y.ch", "password": "sehr-geheim-123"})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // CSRF: ohne Header abgelehnt.
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/sites")
        .header("cookie", cookie.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"domain": "example.com", "timezone": "UTC"}).to_string(),
        ))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))));
    let res = t.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Website anlegen
    let r = t
        .call(
            Method::POST,
            "/api/sites",
            Some(json!({"domain": "https://www.Example.com/", "timezone": "Europe/Zurich"})),
            &auth,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["domain"], "example.com");
    assert!(
        r.json["snippet"]
            .as_str()
            .unwrap()
            .contains("data-domain=\"example.com\"")
    );
    assert!(
        r.json["snippet"]
            .as_str()
            .unwrap()
            .contains("http://analytics.test/js/script.js")
    );

    // Ereignis erfassen (wie das Skript: text/plain, kurze Schluessel)
    let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15";
    let r = t
        .call(
            Method::POST,
            "/api/event",
            Some(json!({"n": "pageview", "u": "https://example.com/hallo?utm_source=test", "d": "example.com", "r": null, "w": 1440})),
            &[("user-agent", ua)],
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(r.headers.get("access-control-allow-origin").unwrap(), "*");
    assert!(r.headers.get("x-plausible-dropped").is_none());
    let r = t
        .call(
            Method::POST,
            "/api/event",
            Some(json!({"n": "pageview", "u": "kaputt", "d": "example.com"})),
            &[("user-agent", ua)],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    t.state.ingest.flush().await;

    // Statistik
    let q = json!({"metrics": ["visitors", "pageviews"], "date_range": "day"});
    let r = t
        .call(
            Method::POST,
            "/api/stats/example.com/query",
            Some(q.clone()),
            &auth,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["results"][0]["metrics"], json!([1, 1]));
    let r = t
        .call(Method::GET, "/api/stats/example.com/realtime", None, &auth)
        .await;
    assert_eq!(r.json["visitors"], 1);

    // Ohne Anmeldung: privat → 401, oeffentlich → erlaubt
    let r = t
        .call(Method::POST, "/api/stats/example.com/query", Some(q.clone()), &[])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = t
        .call(
            Method::PUT,
            "/api/sites/example.com",
            Some(json!({"public": true})),
            &auth,
        )
        .await;
    assert_eq!(r.json["public"], true);
    let r = t
        .call(Method::POST, "/api/stats/example.com/query", Some(q.clone()), &[])
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = t.call(Method::GET, "/api/sites/example.com", None, &[]).await;
    assert!(r.json["role"].is_null());
    t.call(
        Method::PUT,
        "/api/sites/example.com",
        Some(json!({"public": false})),
        &auth,
    )
    .await;

    // Geteilter Link mit Passwort
    let r = t
        .call(
            Method::POST,
            "/api/sites/example.com/shared-links",
            Some(json!({"name": "Kunde", "password": "teilen123"})),
            &auth,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let slug = r.json["slug"].as_str().unwrap().to_string();
    assert!(
        r.json["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/share/example.com?auth={slug}"))
    );
    let share = [("x-nrz-share", slug.as_str())];
    let r = t
        .call(
            Method::POST,
            "/api/stats/example.com/query",
            Some(q.clone()),
            &share,
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.json["code"], "share_password_required");
    let r = t
        .call(Method::GET, &format!("/api/share/{slug}"), None, &[])
        .await;
    assert_eq!(r.json["needs_password"], true);
    let r = t
        .call(
            Method::POST,
            &format!("/api/share/{slug}/auth"),
            Some(json!({"password": "falsch"})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = t
        .call(
            Method::POST,
            &format!("/api/share/{slug}/auth"),
            Some(json!({"password": "teilen123"})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let share_cookie = session_cookie(&r);
    // Mit dem Freigabe-Cookie fragt die Oberflaeche nicht erneut nach dem Passwort.
    let r = t
        .call(
            Method::GET,
            &format!("/api/share/{slug}"),
            None,
            &[("cookie", share_cookie.as_str())],
        )
        .await;
    assert_eq!(r.json["needs_password"], false, "{}", r.text);
    let r = t
        .call(
            Method::POST,
            "/api/stats/example.com/query",
            Some(q.clone()),
            &[("x-nrz-share", slug.as_str()), ("cookie", share_cookie.as_str())],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);

    // Ziel und Konversionsrate
    let r = t
        .call(
            Method::POST,
            "/api/sites/example.com/goals",
            Some(json!({"kind": "page", "page_path": "/hallo"})),
            &auth,
        )
        .await;
    assert_eq!(r.json["display_name"], "Besuch /hallo");
    let r = t
        .call(
            Method::POST,
            "/api/stats/example.com/query",
            Some(json!({"metrics": ["visitors", "conversion_rate"], "date_range": "day", "filters": [["is", "event:goal", ["Besuch /hallo"]]]})),
            &auth,
        )
        .await;
    assert_eq!(r.json["results"][0]["metrics"], json!([1, 100.0]));

    // API-Schluessel: Stats-API v2 und v1, Sites-API braucht Bereich 'sites'
    let r = t
        .call(
            Method::POST,
            "/api/account/api-keys",
            Some(json!({"name": "Test"})),
            &auth,
        )
        .await;
    let key = r.json["key"].as_str().unwrap().to_string();
    let bearer = format!("Bearer {key}");
    let api = [("authorization", bearer.as_str())];
    let r = t
        .call(
            Method::POST,
            "/api/v2/query",
            Some(json!({"site_id": "example.com", "metrics": ["visitors"], "date_range": "day"})),
            &api,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["results"][0]["metrics"][0], 1);
    let r = t
        .call(
            Method::GET,
            "/api/v1/stats/aggregate?site_id=example.com&period=day&metrics=visitors,pageviews",
            None,
            &api,
        )
        .await;
    assert_eq!(r.json["results"]["visitors"]["value"], 1);
    let r = t
        .call(
            Method::GET,
            "/api/v1/stats/breakdown?site_id=example.com&period=day&property=visit:source",
            None,
            &api,
        )
        .await;
    assert_eq!(r.json["results"][0]["source"], "test");
    let r = t
        .call(
            Method::GET,
            "/api/v1/stats/timeseries?site_id=example.com&period=7d",
            None,
            &api,
        )
        .await;
    assert_eq!(r.json["results"].as_array().unwrap().len(), 7);
    let r = t
        .call(
            Method::GET,
            "/api/v1/stats/realtime/visitors?site_id=example.com",
            None,
            &api,
        )
        .await;
    assert_eq!(r.json, json!(1));
    let r = t
        .call(
            Method::POST,
            "/api/v1/sites",
            Some(json!({"domain": "zwei.ch"})),
            &api,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = t
        .call(
            Method::POST,
            "/api/v2/query",
            Some(json!({"site_id": "example.com", "metrics": ["visitors"]})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // MCP
    let r = t
        .call(
            Method::POST,
            "/mcp",
            Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})),
            &api,
        )
        .await;
    assert!(r.json["result"]["tools"].as_array().unwrap().len() >= 10);
    let r = t
        .call(
            Method::POST,
            "/mcp",
            Some(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "aggregate", "arguments": {"site_id": "example.com", "date_range": "day"}}})),
            &api,
        )
        .await;
    assert_eq!(r.json["result"]["isError"], false, "{}", r.text);
    assert_eq!(r.json["result"]["structuredContent"]["rows"][0]["visitors"], 1);

    // Tracking-Skript
    let r = t.call(Method::GET, "/js/script.js", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("javascript")
    );
    let r = t.call(Method::GET, "/js/script.unbekannt.js", None, &[]).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Abmelden
    let r = t.call(Method::POST, "/api/auth/logout", None, &auth).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = t.call(Method::GET, "/api/sites", None, &auth).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_with_totp_and_lockout() {
    let t = test_app().await;
    let r = t
        .call(
            Method::POST,
            "/api/auth/register",
            Some(json!({"email": "nico@example.ch", "password": "sehr-geheim-123"})),
            &[],
        )
        .await;
    let cookie = session_cookie(&r);
    let auth = [("cookie", cookie.as_str())];
    let r = t.call(Method::POST, "/api/account/2fa/setup", None, &auth).await;
    let secret = r.json["secret"].as_str().unwrap().to_string();
    assert!(
        r.json["otpauth_url"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/")
    );
    // Code berechnen wie eine Authenticator-App.
    let code = crate::auth::totp_code(&secret, crate::db::now());
    let r = t
        .call(
            Method::POST,
            "/api/account/2fa/enable",
            Some(json!({"code": code})),
            &auth,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let recovery = r.json["codes"][0].as_str().unwrap().to_string();

    let r = t
        .call(
            Method::POST,
            "/api/auth/login",
            Some(json!({"email": "nico@example.ch", "password": "sehr-geheim-123"})),
            &[],
        )
        .await;
    assert_eq!(r.json["code"], "totp_required");
    let r = t
        .call(
            Method::POST,
            "/api/auth/login",
            Some(json!({"email": "nico@example.ch", "password": "sehr-geheim-123", "totp": recovery})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // Wiederherstellungscode gilt nur einmal.
    let r = t
        .call(
            Method::POST,
            "/api/auth/login",
            Some(json!({"email": "nico@example.ch", "password": "sehr-geheim-123", "totp": recovery})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Sperre nach zu vielen Fehlversuchen
    for _ in 0..8 {
        t.call(
            Method::POST,
            "/api/auth/login",
            Some(json!({"email": "nico@example.ch", "password": "falsch-falsch"})),
            &[],
        )
        .await;
    }
    let r = t
        .call(
            Method::POST,
            "/api/auth/login",
            Some(json!({"email": "nico@example.ch", "password": "sehr-geheim-123"})),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}
