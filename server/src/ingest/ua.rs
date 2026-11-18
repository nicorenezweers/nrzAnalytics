//! User-Agent auswerten: Browser, Betriebssystem, Geraeteart, Bot-Erkennung.
//!
//! Bewusst ohne regulaere Ausdruecke: Die Erkennung prueft feste Kennzeichen (`Edg/`, `CriOS/` …)
//! in einer festen Reihenfolge. Viele Browser geben sich zusaetzlich als Chrome und Safari aus,
//! deshalb kommen spezielle Browser vor Chrome und Chrome vor Safari.

/// Ergebnis der Auswertung eines User-Agent-Strings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserAgent {
    /// Browser, z. B. `Chrome`, `Safari`, `Firefox`, `Microsoft Edge`, `Opera`, `Samsung Internet`.
    /// Leer, wenn unbekannt.
    pub browser: String,
    /// Version `major.minor`, z. B. `131.0`. Leer, wenn unbekannt.
    pub browser_version: String,
    /// Betriebssystem, z. B. `Windows`, `macOS`, `iOS`, `iPadOS`, `Android`, `GNU/Linux`, `ChromeOS`.
    pub os: String,
    /// Version, z. B. `11`, `17.5`, `10.15`. Leer, wenn unbekannt.
    pub os_version: String,
    /// Geraeteart laut User-Agent (gilt nur, wenn keine Bildschirmbreite mitkommt).
    pub form: FormFactor,
    /// Crawler, Bot, Headless-Browser, Monitoring oder HTTP-Bibliothek.
    pub is_bot: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormFactor {
    #[default]
    Desktop,
    Tablet,
    Mobile,
}

/// Wertet einen User-Agent aus.
pub fn parse(ua: &str) -> UserAgent {
    let ua = ua.trim();
    // Uebergrosse Header nicht komplett durchsuchen (echte User-Agents sind < 1 KB).
    let ua = cut(ua, 1024);
    let (browser, browser_version) = detect_browser(ua);
    let (os, os_version) = detect_os(ua);
    UserAgent {
        browser: browser.to_string(),
        browser_version,
        os: os.to_string(),
        os_version,
        form: detect_form(ua),
        is_bot: super::bots::is_bot_user_agent(ua),
    }
}

/// Kuerzt auf hoechstens `max` Bytes an einer Zeichengrenze.
fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Die ersten zwei Zahlen nach `token` (getrennt durch `.` oder `_`), z. B. `131`, `0`.
fn numbers_after<'a>(ua: &'a str, token: &str) -> Option<(&'a str, Option<&'a str>)> {
    let start = ua.find(token)? + token.len();
    numbers_at(&ua[start..])
}

/// Die ersten zwei Zahlen am Anfang von `s`.
fn numbers_at(s: &str) -> Option<(&str, Option<&str>)> {
    let b = s.as_bytes();
    let major_end = b.iter().position(|c| !c.is_ascii_digit()).unwrap_or(b.len());
    if major_end == 0 {
        return None;
    }
    let major = &s[..major_end];
    let minor = match b.get(major_end) {
        Some(b'.' | b'_') => {
            let rest = &s[major_end + 1..];
            let rb = rest.as_bytes();
            let end = rb.iter().position(|c| !c.is_ascii_digit()).unwrap_or(rb.len());
            (end > 0).then(|| &rest[..end])
        }
        _ => None,
    };
    Some((major, minor))
}

/// Browser-Version `major.minor` nach `token` (fehlende Nebenversion → `0`).
fn version(ua: &str, token: &str) -> String {
    match numbers_after(ua, token) {
        Some((major, minor)) => format!("{major}.{}", minor.unwrap_or("0")),
        None => String::new(),
    }
}

/// Betriebssystem-Version nach `token`: `major.minor`, nur `major`, wenn keine Nebenversion da
/// ist (`Android 14` → `14`).
fn os_version(ua: &str, token: &str) -> String {
    match numbers_after(ua, token) {
        Some((major, Some(minor))) => format!("{major}.{minor}"),
        Some((major, None)) => major.to_string(),
        None => String::new(),
    }
}

fn has(ua: &str, token: &str) -> bool {
    ua.contains(token)
}

/// Browser und Version bestimmen.
fn detect_browser(ua: &str) -> (&'static str, String) {
    // In-App-Browser zuerst: Sie enthalten oft zusaetzlich `; wv)`, `Chrome/` oder `Safari/`.
    if has(ua, "Instagram") {
        return ("Instagram", version(ua, "Instagram "));
    }
    if has(ua, "FBAN/") || has(ua, "FBAV/") || has(ua, "FB_IAB") || has(ua, "[FB") {
        return ("Facebook", version(ua, "FBAV/"));
    }
    if has(ua, "musical_ly") || has(ua, "TikTok") || has(ua, "BytedanceWebview") {
        let v = if has(ua, "musical_ly_") {
            version(ua, "musical_ly_")
        } else {
            version(ua, "app_version/")
        };
        return ("TikTok", v);
    }
    if has(ua, "Snapchat") {
        return ("Snapchat", version(ua, "Snapchat/"));
    }
    if has(ua, "Pinterest") {
        return ("Pinterest", version(ua, "Pinterest/"));
    }
    if has(ua, "LinkedInApp") {
        return ("LinkedIn", version(ua, "LinkedInApp/"));
    }
    if has(ua, "WhatsApp") {
        return ("WhatsApp", version(ua, "WhatsApp/"));
    }
    if has(ua, "Line/") {
        return ("Line", version(ua, "Line/"));
    }
    if has(ua, "GSA/") {
        return ("Google App", version(ua, "GSA/"));
    }

    // Browser auf Chromium-Basis bzw. mit eigener Kennung (vor Chrome und Safari pruefen).
    for (token, name) in [
        ("EdgiOS/", "Microsoft Edge"),
        ("EdgA/", "Microsoft Edge"),
        ("Edg/", "Microsoft Edge"),
        ("Edge/", "Microsoft Edge"),
        ("Opera Mini/", "Opera Mini"),
        ("OPiOS/", "Opera"),
        ("OPT/", "Opera"),
        ("OPX/", "Opera"),
        ("OPR/", "Opera"),
        ("SamsungBrowser/", "Samsung Internet"),
        ("YaBrowser/", "Yandex Browser"),
        ("YaSearchBrowser/", "Yandex Browser"),
        ("Vivaldi/", "Vivaldi"),
        ("UCBrowser/", "UC Browser"),
        ("UBrowser/", "UC Browser"),
        ("DuckDuckGo/", "DuckDuckGo"),
        ("Ddg/", "DuckDuckGo"),
        ("Brave/", "Brave"),
        ("HuaweiBrowser/", "Huawei Browser"),
        ("MiuiBrowser/", "MIUI Browser"),
        ("Whale/", "Whale"),
        ("Silk/", "Silk"),
        ("CriOS/", "Chrome"),
        ("FxiOS/", "Firefox"),
        ("Firefox/", "Firefox"),
    ] {
        if has(ua, token) {
            return (name, version(ua, token));
        }
    }
    if has(ua, "Opera/") {
        // Altes Opera (Presto): echte Version steht hinter `Version/`.
        let v = if has(ua, "Version/") {
            version(ua, "Version/")
        } else {
            version(ua, "Opera/")
        };
        return ("Opera", v);
    }
    if has(ua, "Brave") {
        return ("Brave", version(ua, "Chrome/"));
    }
    if has(ua, "Chrome/") {
        // Android-WebView: `; wv)` bzw. `Version/4.0 Chrome/`.
        if has(ua, "Android") && (has(ua, "; wv)") || has(ua, "Version/4.0 Chrome/")) {
            return ("Chrome Webview", version(ua, "Chrome/"));
        }
        return ("Chrome", version(ua, "Chrome/"));
    }
    if has(ua, "HeadlessChrome/") {
        return ("Chrome", version(ua, "HeadlessChrome/"));
    }
    if has(ua, "Chromium/") {
        return ("Chromium", version(ua, "Chromium/"));
    }
    if has(ua, "Safari/") && has(ua, "Version/") {
        return ("Safari", version(ua, "Version/"));
    }
    if has(ua, "Trident/") || has(ua, "MSIE ") {
        let v = if has(ua, "rv:") {
            version(ua, "rv:")
        } else {
            version(ua, "MSIE ")
        };
        return ("Internet Explorer", v);
    }
    // iOS-App mit eingebettetem WebKit (ohne `Safari/`).
    if (has(ua, "iPhone") || has(ua, "iPad") || has(ua, "iPod")) && has(ua, "AppleWebKit/") {
        return ("Safari (in-app)", String::new());
    }
    if has(ua, "Android") && has(ua, "AppleWebKit/") && has(ua, "Version/") {
        // Sehr alter Android-Browser.
        return ("Android Browser", version(ua, "Version/"));
    }
    ("", String::new())
}

/// Windows-NT-Version in die Marketing-Version uebersetzen.
fn windows_version(ua: &str) -> String {
    let v = match numbers_after(ua, "Windows NT ") {
        Some((major, minor)) => (major, minor.unwrap_or("0")),
        None => return String::new(),
    };
    match v {
        // Windows 11 meldet sich ebenfalls als NT 10.0 und ist nicht unterscheidbar.
        ("10", "0") => "10",
        ("6", "3") => "8.1",
        ("6", "2") => "8",
        ("6", "1") => "7",
        ("6", "0") => "Vista",
        ("5", "1" | "2") => "XP",
        _ => "",
    }
    .to_string()
}

/// Betriebssystem und Version bestimmen.
fn detect_os(ua: &str) -> (&'static str, String) {
    if has(ua, "Windows Phone") {
        return ("Windows Phone", os_version(ua, "Windows Phone "));
    }
    if has(ua, "Xbox") {
        return ("Xbox", String::new());
    }
    if has(ua, "PlayStation") {
        return ("PlayStation", os_version(ua, "PlayStation "));
    }
    if has(ua, "Nintendo") {
        return ("Nintendo", String::new());
    }
    if has(ua, "Windows") {
        return ("Windows", windows_version(ua));
    }
    if has(ua, "HarmonyOS") || has(ua, "OpenHarmony") {
        let v = if has(ua, "OpenHarmony ") {
            os_version(ua, "OpenHarmony ")
        } else {
            os_version(ua, "HarmonyOS ")
        };
        return ("HarmonyOS", v);
    }
    if has(ua, "KAIOS") || has(ua, "KaiOS") {
        let v = if has(ua, "KAIOS/") {
            os_version(ua, "KAIOS/")
        } else {
            os_version(ua, "KaiOS/")
        };
        return ("KaiOS", v);
    }
    if has(ua, "Tizen") {
        return ("Tizen", os_version(ua, "Tizen "));
    }
    if has(ua, "Android") {
        // `Android 14`, Firefox: `(Android 14; Mobile; …)`.
        return ("Android", os_version(ua, "Android "));
    }
    if has(ua, "CrOS") {
        // `CrOS x86_64 14541.0.0` – Plattformversion nach der Architektur.
        let v = ua
            .find("CrOS ")
            .and_then(|i| ua[i + 5..].split_once(' '))
            .and_then(|(_, rest)| numbers_at(rest))
            .map(|(major, minor)| format!("{major}.{}", minor.unwrap_or("0")))
            .unwrap_or_default();
        return ("ChromeOS", v);
    }
    if has(ua, "iPad") {
        return ("iPadOS", os_version(ua, " OS "));
    }
    if has(ua, "iPhone") || has(ua, "iPod") {
        return ("iOS", os_version(ua, " OS "));
    }
    if has(ua, "Mac OS X") || has(ua, "Macintosh") {
        return ("macOS", os_version(ua, "Mac OS X "));
    }
    if has(ua, "FreeBSD") {
        return ("FreeBSD", String::new());
    }
    if has(ua, "OpenBSD") {
        return ("OpenBSD", String::new());
    }
    if has(ua, "NetBSD") {
        return ("NetBSD", String::new());
    }
    if has(ua, "Linux") || has(ua, "X11") || has(ua, "Ubuntu") || has(ua, "Fedora") {
        return ("GNU/Linux", String::new());
    }
    ("", String::new())
}

/// Geraeteart laut User-Agent.
fn detect_form(ua: &str) -> FormFactor {
    if has(ua, "iPad") || has(ua, "Tablet") || has(ua, "Kindle") || has(ua, "Silk/") || has(ua, "PlayBook") {
        return FormFactor::Tablet;
    }
    if has(ua, "Mobi")
        || has(ua, "iPhone")
        || has(ua, "iPod")
        || has(ua, "Windows Phone")
        || has(ua, "KAIOS")
        || has(ua, "Opera Mini")
    {
        return FormFactor::Mobile;
    }
    // Fernseher und Konsolen zaehlen als Desktop.
    if has(ua, "Android")
        && !has(ua, "Android TV")
        && !has(ua, "SMART-TV")
        && !has(ua, "SmartTV")
        && !has(ua, "CrKey")
        && !has(ua, "AFT")
    {
        return FormFactor::Tablet;
    }
    FormFactor::Desktop
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prueft Browser, Version, System, Systemversion und Geraeteart.
    fn check(ua: &str, browser: &str, bv: &str, os: &str, ov: &str, form: FormFactor) {
        let r = parse(ua);
        assert_eq!(
            (
                r.browser.as_str(),
                r.browser_version.as_str(),
                r.os.as_str(),
                r.os_version.as_str(),
                r.form
            ),
            (browser, bv, os, ov, form),
            "{ua}"
        );
    }

    use FormFactor::{Desktop, Mobile, Tablet};

    #[test]
    fn chrome_windows() {
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            "Chrome",
            "131.0",
            "Windows",
            "10",
            Desktop,
        );
        assert!(!parse("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36").is_bot);
    }

    #[test]
    fn chrome_windows_7_und_8() {
        check(
            "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/109.0.0.0 Safari/537.36",
            "Chrome",
            "109.0",
            "Windows",
            "7",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Windows NT 6.3; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/109.0.0.0 Safari/537.36",
            "Chrome",
            "109.0",
            "Windows",
            "8.1",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Windows NT 6.2; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/49.0.2623.112 Safari/537.36",
            "Chrome",
            "49.0",
            "Windows",
            "8",
            Desktop,
        );
    }

    #[test]
    fn chrome_mac() {
        check(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            "Chrome",
            "131.0",
            "macOS",
            "10.15",
            Desktop,
        );
    }

    #[test]
    fn chrome_linux() {
        check(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            "Chrome",
            "131.0",
            "GNU/Linux",
            "",
            Desktop,
        );
    }

    #[test]
    fn chrome_android() {
        check(
            "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
            "Chrome",
            "131.0",
            "Android",
            "10",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Linux; Android 14; Pixel 8 Pro) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.6723.102 Mobile Safari/537.36",
            "Chrome",
            "130.0",
            "Android",
            "14",
            Mobile,
        );
    }

    #[test]
    fn chrome_android_tablet() {
        check(
            "Mozilla/5.0 (Linux; Android 13; SM-X710) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            "Chrome",
            "131.0",
            "Android",
            "13",
            Tablet,
        );
    }

    #[test]
    fn chrome_ios() {
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0.6778.73 Mobile/15E148 Safari/604.1",
            "Chrome",
            "131.0",
            "iOS",
            "18.1",
            Mobile,
        );
    }

    #[test]
    fn chromeos() {
        check(
            "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            "Chrome",
            "131.0",
            "ChromeOS",
            "14541.0",
            Desktop,
        );
    }

    #[test]
    fn safari_mac() {
        check(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15",
            "Safari",
            "18.1",
            "macOS",
            "10.15",
            Desktop,
        );
    }

    #[test]
    fn safari_ios() {
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
            "Safari",
            "17.5",
            "iOS",
            "17.5",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1.1 Mobile/15E148 Safari/604.1",
            "Safari",
            "18.1",
            "iOS",
            "18.1",
            Mobile,
        );
    }

    #[test]
    fn safari_ipad_alt() {
        check(
            "Mozilla/5.0 (iPad; CPU OS 12_5_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/12.1.2 Mobile/15E148 Safari/604.1",
            "Safari",
            "12.1",
            "iPadOS",
            "12.5",
            Tablet,
        );
        check(
            "Mozilla/5.0 (iPad; CPU OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1",
            "Safari",
            "17.6",
            "iPadOS",
            "17.6",
            Tablet,
        );
    }

    #[test]
    fn ipados_als_mac() {
        // iPadOS 13+ meldet sich im Desktop-Modus als Mac.
        check(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
            "Safari",
            "17.5",
            "macOS",
            "10.15",
            Desktop,
        );
    }

    #[test]
    fn firefox_desktop() {
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0",
            "Firefox",
            "132.0",
            "Windows",
            "10",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:132.0) Gecko/20100101 Firefox/132.0",
            "Firefox",
            "132.0",
            "macOS",
            "10.15",
            Desktop,
        );
        check(
            "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0",
            "Firefox",
            "132.0",
            "GNU/Linux",
            "",
            Desktop,
        );
        check(
            "Mozilla/5.0 (X11; Fedora; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0",
            "Firefox",
            "131.0",
            "GNU/Linux",
            "",
            Desktop,
        );
    }

    #[test]
    fn firefox_mobil() {
        check(
            "Mozilla/5.0 (Android 14; Mobile; rv:132.0) Gecko/132.0 Firefox/132.0",
            "Firefox",
            "132.0",
            "Android",
            "14",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Android 13; Tablet; rv:132.0) Gecko/132.0 Firefox/132.0",
            "Firefox",
            "132.0",
            "Android",
            "13",
            Tablet,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/132.0 Mobile/15E148 Safari/605.1.15",
            "Firefox",
            "132.0",
            "iOS",
            "18.1",
            Mobile,
        );
    }

    #[test]
    fn firefox_freebsd() {
        check(
            "Mozilla/5.0 (X11; FreeBSD amd64; rv:128.0) Gecko/20100101 Firefox/128.0",
            "Firefox",
            "128.0",
            "FreeBSD",
            "",
            Desktop,
        );
    }

    #[test]
    fn edge() {
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.2903.51",
            "Microsoft Edge",
            "131.0",
            "Windows",
            "10",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.2903.51",
            "Microsoft Edge",
            "131.0",
            "macOS",
            "10.15",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36 EdgA/131.0.2903.48",
            "Microsoft Edge",
            "131.0",
            "Android",
            "10",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 EdgiOS/131.2903.45 Mobile/15E148 Safari/605.1.15",
            "Microsoft Edge",
            "131.2903",
            "iOS",
            "18.1",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/70.0.3538.102 Safari/537.36 Edge/18.19582",
            "Microsoft Edge",
            "18.19582",
            "Windows",
            "10",
            Desktop,
        );
    }

    #[test]
    fn opera() {
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 OPR/115.0.0.0",
            "Opera",
            "115.0",
            "Windows",
            "10",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36 OPR/86.0.0.0",
            "Opera",
            "86.0",
            "Android",
            "10",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) OPiOS/16.0.15.124050 Mobile/15E148 Safari/9537.53",
            "Opera",
            "16.0",
            "iOS",
            "17.5",
            Mobile,
        );
        check(
            "Opera/9.80 (Android; Opera Mini/36.2.2254/119.132; U; id) Presto/2.12.423 Version/12.16",
            "Opera Mini",
            "36.2",
            "Android",
            "",
            Mobile,
        );
    }

    #[test]
    fn samsung_internet() {
        check(
            "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
            "Samsung Internet",
            "26.0",
            "Android",
            "14",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Linux; Android 14; SM-X810) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Safari/537.36",
            "Samsung Internet",
            "26.0",
            "Android",
            "14",
            Tablet,
        );
    }

    #[test]
    fn yandex_vivaldi_uc_whale() {
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 YaBrowser/24.10.0.0 Safari/537.36",
            "Yandex Browser",
            "24.10",
            "Windows",
            "10",
            Desktop,
        );
        check(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Vivaldi/7.0.3495.15",
            "Vivaldi",
            "7.0",
            "GNU/Linux",
            "",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Linux; U; Android 13; en-US; V2250 Build/TP1A.220624.014) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/123.0.6312.80 UCBrowser/13.7.5.1321 Mobile Safari/537.36",
            "UC Browser",
            "13.7",
            "Android",
            "13",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Whale/3.28.266.14 Safari/537.36",
            "Whale",
            "3.28",
            "Windows",
            "10",
            Desktop,
        );
    }

    #[test]
    fn duckduckgo_und_brave() {
        check(
            "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/131.0.0.0 Mobile DuckDuckGo/5 Safari/537.36",
            "DuckDuckGo",
            "5.0",
            "Android",
            "14",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Ddg/17.5 Safari/605.1.15",
            "DuckDuckGo",
            "17.5",
            "iOS",
            "17.5",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Brave/131",
            "Brave",
            "131.0",
            "Windows",
            "10",
            Desktop,
        );
    }

    #[test]
    fn huawei_miui_silk() {
        check(
            "Mozilla/5.0 (Linux; Android 12; HarmonyOS; NOH-AN01; HMSCore 6.13.0.302) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.5735.196 HuaweiBrowser/15.0.4.312 Mobile Safari/537.36",
            "Huawei Browser",
            "15.0",
            "HarmonyOS",
            "",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Linux; U; Android 13; de-de; 2201117TG Build/TKQ1.221114.001) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/112.0.5615.136 Mobile Safari/537.36 XiaoMi/MiuiBrowser/14.8.1-gn",
            "MIUI Browser",
            "14.8",
            "Android",
            "13",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Linux; Android 9; KFTRWI) AppleWebKit/537.36 (KHTML, like Gecko) Silk/130.4.1 like Chrome/130.0.6723.102 Safari/537.36",
            "Silk",
            "130.4",
            "Android",
            "9",
            Tablet,
        );
    }

    #[test]
    fn instagram_in_app() {
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Instagram 352.0.0.29.88 (iPhone15,3; iOS 17_6; de_DE; de; scale=3.00; 1290x2796; 646588129)",
            "Instagram",
            "352.0",
            "iOS",
            "17.6",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Linux; Android 14; SM-A546B Build/UP1A.231005.007; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/130.0.6723.107 Mobile Safari/537.36 Instagram 355.0.0.37.103 Android (34/14; 450dpi; 1080x2125; samsung; SM-A546B; a54x; s5e8835; de_DE; 657465400)",
            "Instagram",
            "355.0",
            "Android",
            "14",
            Mobile,
        );
    }

    #[test]
    fn facebook_in_app() {
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 [FBAN/FBIOS;FBAV/485.0.0.43.109;FBBV/648016733;FBDV/iPhone14,5;FBMD/iPhone;FBSN/iOS;FBSV/17.6.1;FBSS/3;FBID/phone;FBLC/de_DE;FBOP/5;FBRV/650116029]",
            "Facebook",
            "485.0",
            "iOS",
            "17.6",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Linux; Android 14; Pixel 7 Build/AP2A.240905.003; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/129.0.6668.100 Mobile Safari/537.36 [FB_IAB/FB4A;FBAV/484.0.0.62.110;]",
            "Facebook",
            "484.0",
            "Android",
            "14",
            Mobile,
        );
    }

    #[test]
    fn weitere_in_app() {
        check(
            "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP2A.240905.003; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/130.0.6723.102 Mobile Safari/537.36 [Pinterest/Android]",
            "Pinterest",
            "",
            "Android",
            "14",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 musical_ly_36.5.0 JsSdk/2.0 NetType/WIFI Channel/App Store ByteLocale/de Region/DE",
            "TikTok",
            "36.5",
            "iOS",
            "17.5",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Snapchat/13.13.0.38 (like Safari/8618.2.12.10.9, panda)",
            "Snapchat",
            "13.13",
            "iOS",
            "17.5",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 [LinkedInApp]/9.30.1424",
            "LinkedIn",
            "",
            "iOS",
            "17.5",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) GSA/343.0.695551749 Mobile/15E148 Safari/604.1",
            "Google App",
            "343.0",
            "iOS",
            "18.1",
            Mobile,
        );
    }

    #[test]
    fn webviews() {
        check(
            "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP2A.240905.003; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/130.0.6723.102 Mobile Safari/537.36",
            "Chrome Webview",
            "130.0",
            "Android",
            "14",
            Mobile,
        );
        check(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
            "Safari (in-app)",
            "",
            "iOS",
            "17.5",
            Mobile,
        );
    }

    #[test]
    fn exoten() {
        check(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; Xbox; Xbox Series X) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/48.0.2564.82 Safari/537.36 Edge/20.02",
            "Microsoft Edge",
            "20.02",
            "Xbox",
            "",
            Desktop,
        );
        check(
            "Mozilla/5.0 (PlayStation 5 6.50) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/13.0 Safari/605.1.15",
            "Safari",
            "13.0",
            "PlayStation",
            "5",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Mobile; LYF/F300B/LYF-F300B-001-01-15-130718-i;Android; rv:48.0) Gecko/48.0 Firefox/48.0 KAIOS/2.5",
            "Firefox",
            "48.0",
            "KaiOS",
            "2.5",
            Mobile,
        );
        check(
            "Mozilla/5.0 (SMART-TV; LINUX; Tizen 6.0) AppleWebKit/537.36 (KHTML, like Gecko) 76.0.3809.146/6.0 TV Safari/537.36",
            "",
            "",
            "Tizen",
            "6.0",
            Desktop,
        );
        check(
            "Mozilla/5.0 (Windows Phone 10.0; Android 6.0.1; Microsoft; Lumia 950) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/52.0.2743.116 Mobile Safari/537.36 Edge/15.15063",
            "Microsoft Edge",
            "15.15063",
            "Windows Phone",
            "10.0",
            Mobile,
        );
        check(
            "Mozilla/5.0 (Windows NT 10.0; WOW64; Trident/7.0; rv:11.0) like Gecko",
            "Internet Explorer",
            "11.0",
            "Windows",
            "10",
            Desktop,
        );
    }

    #[test]
    fn bots_und_unbekanntes() {
        let g = parse("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)");
        assert!(g.is_bot);
        let c = parse("curl/8.7.1");
        assert!(c.is_bot);
        assert_eq!((c.browser.as_str(), c.os.as_str(), c.form), ("", "", Desktop));
        let h = parse(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/131.0.0.0 Safari/537.36",
        );
        assert!(h.is_bot);
        assert_eq!(h.browser, "Chrome");
        let e = parse("");
        assert!(e.is_bot);
        assert_eq!(
            e,
            UserAgent {
                is_bot: true,
                ..UserAgent::default()
            }
        );
        let x = parse("Irgendwas/1.0 ohne bekannte Kennung");
        assert_eq!((x.browser.as_str(), x.os.as_str()), ("", ""));
    }

    #[test]
    fn robust_bei_muell() {
        let lang = "Mozilla/5.0 ".repeat(500) + "Chrome/";
        let r = parse(&lang);
        assert_eq!(r.browser_version, "");
        let r = parse("Mozilla/5.0 (Windows NT ; ä) Chrome/ä Safari/");
        assert_eq!(
            (
                r.browser.as_str(),
                r.browser_version.as_str(),
                r.os_version.as_str()
            ),
            ("Chrome", "", "")
        );
        let r = parse(&"ä".repeat(600));
        assert_eq!(r.browser, "");
    }
}
