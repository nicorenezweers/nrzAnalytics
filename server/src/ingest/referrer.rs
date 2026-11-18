//! Verweis-URLs (Referrer) auswerten und Quellen benennen.
//!
//! Die Quellen-Tabelle ist an die Snowplow-Referer-Datenbank (die auch Plausible nutzt) und die
//! Quellen-Kategorien des GA4 Default Channel Grouping angelehnt. Die Suche laeuft ueber eine
//! HashMap mit Suffix-Suche (`de.search.yahoo.com` → `yahoo.com`) und eine Laenderdomain-Regel
//! (`google.co.jp` → `google.com`).

use std::collections::HashMap;
use std::sync::LazyLock;

/// Ausgewerteter Verweis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Referrer {
    /// Bereinigte URL ohne Query und Fragment, z. B. `https://news.ycombinator.com/item`.
    pub url: String,
    /// Hostname klein und ohne `www.`, z. B. `news.ycombinator.com`.
    pub host: String,
    /// Bekannte Quelle (`Google`, `Hacker News`) oder sonst der Hostname.
    pub source: String,
}

/// Kategorie einer bekannten Quelle (fuer die Kanal-Bestimmung).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCategory {
    Search,
    Social,
    Video,
    Shopping,
    Email,
}

/// Hoechstlaenge der gespeicherten Verweis-URL.
const MAX_URL: usize = 2000;
/// Hoechstlaenge einer Quelle aus URL-Parametern.
const MAX_PARAM: usize = 300;

use SourceCategory::{Email as E, Search as S, Shopping as P, Social as O, Video as V};

/// Bekannte Quellen: Name, Kategorie, Hostnamen (Subdomains werden mit erkannt).
const SOURCES: &[(&str, Option<SourceCategory>, &[&str])] = &[
    // --- Suchmaschinen ---
    ("Google", Some(S), &["google.com"]),
    ("Bing", Some(S), &["bing.com", "bing.net"]),
    (
        "DuckDuckGo",
        Some(S),
        &["duckduckgo.com", "duck.com", "duckduckgo.onion"],
    ),
    ("Yahoo!", Some(S), &["yahoo.com", "yahoo.net"]),
    ("Yandex", Some(S), &["yandex.com", "ya.ru", "yandex.net"]),
    ("Baidu", Some(S), &["baidu.com", "m.baidu.com"]),
    ("Ecosia", Some(S), &["ecosia.org"]),
    ("Qwant", Some(S), &["qwant.com", "qwantjunior.com"]),
    ("Startpage", Some(S), &["startpage.com", "ixquick.com"]),
    ("Brave Search", Some(S), &["search.brave.com"]),
    ("Kagi", Some(S), &["kagi.com"]),
    ("Ask", Some(S), &["ask.com"]),
    ("AOL", Some(S), &["aol.com", "search.aol.com"]),
    ("Naver", Some(S), &["naver.com", "search.naver.com"]),
    ("Seznam", Some(S), &["seznam.cz", "search.seznam.cz"]),
    ("Swisscows", Some(S), &["swisscows.com", "swisscows.ch"]),
    ("Mojeek", Some(S), &["mojeek.com", "mojeek.co.uk"]),
    ("MetaGer", Some(S), &["metager.org", "metager.de"]),
    ("Perplexity", Some(S), &["perplexity.ai"]),
    ("You.com", Some(S), &["you.com"]),
    ("Phind", Some(S), &["phind.com"]),
    ("Sogou", Some(S), &["sogou.com"]),
    ("360 Search", Some(S), &["so.com"]),
    ("Daum", Some(S), &["daum.net", "search.daum.net"]),
    ("Petal Search", Some(S), &["petalsearch.com"]),
    ("Presearch", Some(S), &["presearch.com", "presearch.org"]),
    ("Lilo", Some(S), &["lilo.org"]),
    ("Dogpile", Some(S), &["dogpile.com"]),
    ("Lycos", Some(S), &["lycos.com"]),
    ("Yep", Some(S), &["yep.com"]),
    ("Marginalia", Some(S), &["search.marginalia.nu"]),
    ("Fireball", Some(S), &["fireball.de", "fireball.com"]),
    ("T-Online", Some(S), &["suche.t-online.de"]),
    ("Web.de Suche", Some(S), &["suche.web.de"]),
    (
        "GMX Suche",
        Some(S),
        &["suche.gmx.net", "suche.gmx.at", "suche.gmx.ch", "search.gmx.com"],
    ),
    // --- KI-Assistenten ---
    ("ChatGPT", None, &["chatgpt.com", "chat.openai.com"]),
    ("Claude", None, &["claude.ai"]),
    ("Gemini", None, &["gemini.google.com", "bard.google.com"]),
    (
        "Copilot",
        None,
        &["copilot.microsoft.com", "copilot.cloud.microsoft"],
    ),
    ("Mistral", None, &["chat.mistral.ai"]),
    ("DeepSeek", None, &["chat.deepseek.com"]),
    ("Grok", None, &["grok.com"]),
    // --- Soziale Netzwerke ---
    (
        "Facebook",
        Some(O),
        &["facebook.com", "fb.com", "fb.me", "facebook.net", "fb.watch"],
    ),
    ("Facebook Messenger", Some(O), &["messenger.com"]),
    ("Instagram", Some(O), &["instagram.com", "ig.me"]),
    (
        "Twitter",
        Some(O),
        &["twitter.com", "t.co", "x.com", "tweetdeck.com"],
    ),
    ("LinkedIn", Some(O), &["linkedin.com", "lnkd.in"]),
    ("Reddit", Some(O), &["reddit.com", "redd.it", "reddit.app.link"]),
    ("Pinterest", Some(O), &["pinterest.com", "pin.it"]),
    ("TikTok", Some(O), &["tiktok.com", "vm.tiktok.com"]),
    ("Snapchat", Some(O), &["snapchat.com"]),
    ("Threads", Some(O), &["threads.net", "threads.com"]),
    (
        "Mastodon",
        Some(O),
        &[
            "mastodon.social",
            "mastodon.online",
            "mastodon.world",
            "mastodon.cloud",
            "mastodon.green",
            "mastodon.art",
            "mastodon.de",
            "mastodon.uno",
            "mstdn.social",
            "mstdn.jp",
            "mas.to",
            "fosstodon.org",
            "hachyderm.io",
            "infosec.exchange",
            "chaos.social",
            "troet.cafe",
            "norden.social",
            "det.social",
            "ruhr.social",
            "social.tchncs.de",
            "techhub.social",
            "universeodon.com",
            "sueden.social",
            "muenchen.social",
            "nrw.social",
            "social.vivaldi.net",
            "mastodontech.de",
            "toot.community",
            "aus.social",
            "masto.ai",
            "indieweb.social",
            "ioc.exchange",
            "kolektiva.social",
        ],
    ),
    ("Bluesky", Some(O), &["bsky.app", "bsky.social"]),
    (
        "Lemmy",
        Some(O),
        &["lemmy.world", "lemmy.ml", "feddit.org", "feddit.de"],
    ),
    ("Tumblr", Some(O), &["tumblr.com", "t.umblr.com"]),
    ("VK", Some(O), &["vk.com", "vk.ru", "vkontakte.ru"]),
    ("Odnoklassniki", Some(O), &["ok.ru"]),
    ("Xing", Some(O), &["xing.com"]),
    (
        "Hacker News",
        Some(O),
        &["news.ycombinator.com", "hn.algolia.com"],
    ),
    ("Product Hunt", Some(O), &["producthunt.com"]),
    ("Lobsters", Some(O), &["lobste.rs"]),
    (
        "Discord",
        Some(O),
        &["discord.com", "discordapp.com", "discord.gg"],
    ),
    ("Telegram", Some(O), &["t.me", "telegram.org", "telegram.me"]),
    ("WhatsApp", Some(O), &["whatsapp.com", "wa.me"]),
    ("Slack", Some(O), &["slack.com"]),
    ("Signal", Some(O), &["signal.org", "signal.me"]),
    (
        "Microsoft Teams",
        Some(O),
        &["teams.microsoft.com", "teams.live.com"],
    ),
    ("Quora", Some(O), &["quora.com"]),
    ("Weibo", Some(O), &["weibo.com", "weibo.cn"]),
    ("WeChat", Some(O), &["weixin.qq.com", "wechat.com"]),
    ("Line", Some(O), &["line.me"]),
    ("Nextdoor", Some(O), &["nextdoor.com"]),
    ("Meetup", Some(O), &["meetup.com"]),
    ("Flickr", Some(O), &["flickr.com"]),
    ("DeviantArt", Some(O), &["deviantart.com"]),
    ("Goodreads", Some(O), &["goodreads.com"]),
    ("Yelp", Some(O), &["yelp.com", "yelp.de"]),
    ("Disqus", Some(O), &["disqus.com"]),
    ("Medium", Some(O), &["medium.com"]),
    ("Flipboard", Some(O), &["flipboard.com"]),
    ("Naver Blog", Some(O), &["blog.naver.com"]),
    ("Zhihu", Some(O), &["zhihu.com"]),
    ("Douban", Some(O), &["douban.com"]),
    ("Kakao", Some(O), &["kakao.com"]),
    ("Behance", Some(O), &["behance.net"]),
    ("Dribbble", Some(O), &["dribbble.com"]),
    ("Stack Overflow", None, &["stackoverflow.com"]),
    (
        "Stack Exchange",
        None,
        &[
            "stackexchange.com",
            "superuser.com",
            "serverfault.com",
            "askubuntu.com",
        ],
    ),
    // --- Video ---
    (
        "YouTube",
        Some(V),
        &["youtube.com", "youtu.be", "youtube-nocookie.com"],
    ),
    ("Vimeo", Some(V), &["vimeo.com"]),
    ("Twitch", Some(V), &["twitch.tv"]),
    ("Dailymotion", Some(V), &["dailymotion.com", "dai.ly"]),
    ("Rumble", Some(V), &["rumble.com"]),
    ("Bilibili", Some(V), &["bilibili.com"]),
    ("TED", Some(V), &["ted.com"]),
    ("Kick", Some(V), &["kick.com"]),
    ("PeerTube", Some(V), &["joinpeertube.org", "framatube.org"]),
    // --- E-Mail-Dienste ---
    ("Gmail", Some(E), &["mail.google.com", "inbox.google.com"]),
    (
        "Outlook",
        Some(E),
        &[
            "outlook.live.com",
            "outlook.office.com",
            "outlook.office365.com",
            "outlook.com",
            "mail.live.com",
        ],
    ),
    ("Yahoo! Mail", Some(E), &["mail.yahoo.com"]),
    (
        "GMX",
        Some(E),
        &["gmx.net", "gmx.de", "gmx.at", "gmx.ch", "gmx.com"],
    ),
    ("Web.de", Some(E), &["web.de"]),
    (
        "Proton Mail",
        Some(E),
        &["mail.proton.me", "mail.protonmail.com", "protonmail.com"],
    ),
    ("iCloud Mail", Some(E), &["icloud.com"]),
    ("Yandex Mail", Some(E), &["mail.yandex.com"]),
    ("AOL Mail", Some(E), &["mail.aol.com"]),
    ("Fastmail", Some(E), &["fastmail.com", "app.fastmail.com"]),
    ("Tuta", Some(E), &["app.tuta.com", "mail.tutanota.com"]),
    ("Zoho Mail", Some(E), &["mail.zoho.com", "mail.zoho.eu"]),
    ("Posteo", Some(E), &["posteo.de"]),
    ("mailbox.org", Some(E), &["mailbox.org"]),
    ("T-Online Mail", Some(E), &["email.t-online.de"]),
    ("Freenet Mail", Some(E), &["webmail.freenet.de"]),
    ("Mail.ru", Some(E), &["e.mail.ru"]),
    // --- Shopping ---
    (
        "Amazon",
        Some(P),
        &["amazon.com", "amzn.to", "amzn.eu", "smile.amazon.com"],
    ),
    ("eBay", Some(P), &["ebay.com", "ebay.us"]),
    ("Etsy", Some(P), &["etsy.com", "etsy.me"]),
    ("Google Shopping", Some(P), &["shopping.google.com"]),
    ("Idealo", Some(P), &["idealo.com", "idealo.de"]),
    ("Otto", Some(P), &["otto.de"]),
    ("Zalando", Some(P), &["zalando.com"]),
    ("AliExpress", Some(P), &["aliexpress.com"]),
    ("Alibaba", Some(P), &["alibaba.com"]),
    ("Walmart", Some(P), &["walmart.com"]),
    ("Temu", Some(P), &["temu.com"]),
    (
        "Kleinanzeigen",
        Some(P),
        &["kleinanzeigen.de", "ebay-kleinanzeigen.de"],
    ),
    (
        "Geizhals",
        Some(P),
        &["geizhals.com", "geizhals.de", "geizhals.at"],
    ),
    ("billiger.de", Some(P), &["billiger.de"]),
    ("Rakuten", Some(P), &["rakuten.com"]),
    ("Mercado Libre", Some(P), &["mercadolibre.com"]),
    ("Shopify", Some(P), &["shopify.com", "shop.app"]),
    ("Target", Some(P), &["target.com"]),
    ("Best Buy", Some(P), &["bestbuy.com"]),
    // --- Sonstige ---
    ("GitHub", None, &["github.com"]),
    ("GitLab", None, &["gitlab.com"]),
    ("Codeberg", None, &["codeberg.org"]),
    ("Substack", None, &["substack.com"]),
    ("Wikipedia", None, &["wikipedia.org"]),
    ("Dev.to", None, &["dev.to"]),
    ("Hashnode", None, &["hashnode.com", "hashnode.dev"]),
    ("Feedly", None, &["feedly.com"]),
    ("Inoreader", None, &["inoreader.com"]),
    ("NewsBlur", None, &["newsblur.com"]),
    ("Google News", None, &["news.google.com"]),
    ("Google Docs", None, &["docs.google.com"]),
    ("Google Play", None, &["play.google.com"]),
    ("App Store", None, &["apps.apple.com", "itunes.apple.com"]),
    ("Notion", None, &["notion.so", "notion.site"]),
    ("Trustpilot", None, &["trustpilot.com"]),
    ("Glassdoor", None, &["glassdoor.com", "glassdoor.de"]),
    ("Indeed", None, &["indeed.com"]),
    ("Kununu", None, &["kununu.com"]),
    ("MSN", None, &["msn.com"]),
    ("Pocket", None, &["getpocket.com"]),
    ("Archive.org", None, &["archive.org", "web.archive.org"]),
    ("Bitly", None, &["bit.ly"]),
];

/// Marken mit vielen Laenderdomains: `google.de`, `amazon.co.uk` … werden auf `<marke>.com`
/// abgebildet.
const CCTLD_BRANDS: &[&str] = &[
    "google",
    "yahoo",
    "yandex",
    "bing",
    "amazon",
    "ebay",
    "baidu",
    "pinterest",
    "zalando",
    "aliexpress",
    "rakuten",
    "mercadolibre",
    "idealo",
    "startpage",
    "linkedin",
    "tiktok",
];

/// Zweite Ebene vor Laenderdomains (`co.uk`, `com.br` …).
const SECOND_LEVEL: &[&str] = &["co", "com", "net", "org", "ne", "or", "ac", "gov", "edu", "gob"];

/// Bekannte Android-Apps als Verweis (`android-app://<paket>`).
const ANDROID_APPS: &[(&str, &str)] = &[
    ("com.google.android.gm", "Gmail"),
    ("com.google.android.googlequicksearchbox", "Google"),
    ("com.google.android.apps.searchlite", "Google"),
    ("com.google.android.youtube", "YouTube"),
    ("com.google.android.apps.messaging", "Google Messages"),
    ("com.google.android.apps.docs", "Google Docs"),
    ("com.facebook.katana", "Facebook"),
    ("com.facebook.lite", "Facebook"),
    ("com.facebook.orca", "Facebook Messenger"),
    ("com.instagram.android", "Instagram"),
    ("com.twitter.android", "Twitter"),
    ("com.linkedin.android", "LinkedIn"),
    ("com.reddit.frontpage", "Reddit"),
    ("com.pinterest", "Pinterest"),
    ("com.zhiliaoapp.musically", "TikTok"),
    ("com.ss.android.ugc.trill", "TikTok"),
    ("com.snapchat.android", "Snapchat"),
    ("com.whatsapp", "WhatsApp"),
    ("org.telegram.messenger", "Telegram"),
    ("org.thoughtcrime.securesms", "Signal"),
    ("com.slack", "Slack"),
    ("com.discord", "Discord"),
    ("com.microsoft.office.outlook", "Outlook"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.yahoo.mobile.client.android.mail", "Yahoo! Mail"),
    ("de.gmx.mobile.android.mail", "GMX"),
    ("de.web.mobile.android.mail", "Web.de"),
    ("ch.protonmail.android", "Proton Mail"),
    ("com.samsung.android.email.provider", "Samsung Email"),
    ("com.duckduckgo.mobile.android", "DuckDuckGo"),
    ("org.mozilla.firefox", "Firefox"),
    ("com.feedly.android", "Feedly"),
    ("flipboard.app", "Flipboard"),
    ("org.joinmastodon.android", "Mastodon"),
    ("xyz.blueskyweb.app", "Bluesky"),
];

/// Hostname → (Quelle, Kategorie).
static HOSTS: LazyLock<HashMap<&'static str, (&'static str, Option<SourceCategory>)>> = LazyLock::new(|| {
    let mut m = HashMap::with_capacity(400);
    for (name, cat, hosts) in SOURCES {
        for h in *hosts {
            m.insert(*h, (*name, *cat));
        }
    }
    m
});

/// Quellenname klein → (Quelle, Kategorie). Enthaelt auch die Namen der Android-Apps.
static NAMES: LazyLock<HashMap<String, (&'static str, Option<SourceCategory>)>> = LazyLock::new(|| {
    let mut m = HashMap::with_capacity(250);
    for (name, cat, _) in SOURCES {
        m.insert(name.to_lowercase(), (*name, *cat));
    }
    for (_, name) in ANDROID_APPS {
        m.entry(name.to_lowercase()).or_insert((*name, None));
    }
    m.entry("samsung email".into())
        .or_insert(("Samsung Email", Some(E)));
    m.insert("google messages".into(), ("Google Messages", Some(O)));
    m
});

/// Gaengige Kurzformen in `utm_source` (klein) → Quelle.
const ALIASES: &[(&str, &str)] = &[
    ("fb", "Facebook"),
    ("meta", "Facebook"),
    ("ig", "Instagram"),
    ("insta", "Instagram"),
    ("tw", "Twitter"),
    ("x", "Twitter"),
    ("li", "LinkedIn"),
    ("yt", "YouTube"),
    ("ddg", "DuckDuckGo"),
    ("duck duck go", "DuckDuckGo"),
    ("yahoo", "Yahoo!"),
    ("hn", "Hacker News"),
    ("hackernews", "Hacker News"),
    ("hacker_news", "Hacker News"),
    ("ycombinator", "Hacker News"),
    ("producthunt", "Product Hunt"),
    ("product_hunt", "Product Hunt"),
    ("ph", "Product Hunt"),
    ("bsky", "Bluesky"),
    ("tiktok", "TikTok"),
    ("tt", "TikTok"),
    ("wa", "WhatsApp"),
    ("tg", "Telegram"),
    ("gmail", "Gmail"),
    ("googlemail", "Gmail"),
    ("outlook", "Outlook"),
    ("hotmail", "Outlook"),
    ("protonmail", "Proton Mail"),
    ("ms teams", "Microsoft Teams"),
    ("teams", "Microsoft Teams"),
    ("stackoverflow", "Stack Overflow"),
    ("brave", "Brave Search"),
    ("openai", "ChatGPT"),
    ("chat.openai", "ChatGPT"),
    ("perplexity.ai", "Perplexity"),
    ("bard", "Gemini"),
    ("google-play", "Google Play"),
    ("googleplay", "Google Play"),
    ("appstore", "App Store"),
    ("app store", "App Store"),
    ("adwords", "Google"),
    ("googleads", "Google"),
    ("google_ads", "Google"),
    ("google ads", "Google"),
    ("bingads", "Bing"),
    ("microsoft", "Bing"),
    ("facebook ads", "Facebook"),
    ("facebook_ads", "Facebook"),
    ("instagram_ads", "Instagram"),
];

static ALIAS_MAP: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| ALIASES.iter().copied().collect());

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

/// Hostname vereinheitlichen: klein, ohne `www.` und ohne abschliessenden Punkt.
fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match h.strip_prefix("www.") {
        Some(rest) => rest.to_string(),
        None => h,
    }
}

/// Wertet `referrer` aus. `None`, wenn leer, ungueltig, kein http(s) oder intern (gleicher Host wie
/// `page_host`, `www.` wird ignoriert). `android-app://<paket>` ergibt die App als Quelle.
pub fn parse(referrer: &str, page_host: &str) -> Option<Referrer> {
    let raw = referrer.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("null") || raw.eq_ignore_ascii_case("undefined") {
        return None;
    }
    // Sehr lange Werte nur so weit lesen, wie sie spaeter gespeichert werden koennen (+ Puffer).
    let u = url::Url::parse(cut(raw, MAX_URL * 4)).ok()?;
    match u.scheme() {
        "http" | "https" => {}
        "android-app" => return android_app(&u),
        _ => return None,
    }
    let host_raw = u.host_str()?;
    let host = normalize_host(host_raw);
    if host.is_empty() || host == normalize_host(page_host) {
        return None;
    }
    let source = source_for_host(&host)
        .map(str::to_string)
        .unwrap_or_else(|| host.clone());
    let mut url = String::with_capacity(u.scheme().len() + 3 + host_raw.len() + u.path().len());
    url.push_str(u.scheme());
    url.push_str("://");
    url.push_str(host_raw);
    if let Some(port) = u.port() {
        url.push(':');
        url.push_str(&port.to_string());
    }
    url.push_str(u.path());
    let url = cut(&url, MAX_URL).to_string();
    Some(Referrer { url, host, source })
}

/// Verweis aus einer Android-App (`android-app://com.google.android.gm/…`).
fn android_app(u: &url::Url) -> Option<Referrer> {
    let package = u.host_str()?.trim().to_ascii_lowercase();
    if package.is_empty() {
        return None;
    }
    let source = ANDROID_APPS
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case(&package))
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| package.clone());
    Some(Referrer {
        url: cut(&format!("android-app://{package}"), MAX_URL).to_string(),
        host: package,
        source,
    })
}

/// Eintrag fuer den Hostnamen oder eine seiner uebergeordneten Domains.
fn lookup_suffix(host: &str) -> Option<(&'static str, Option<SourceCategory>)> {
    let mut h = host;
    loop {
        if let Some(v) = HOSTS.get(h) {
            return Some(*v);
        }
        h = &h[h.find('.')? + 1..];
    }
}

/// Laenderdomain einer bekannten Marke auf `.com` abbilden: `news.google.co.uk` →
/// `news.google.com`. `None`, wenn keine Marke erkannt wird.
fn canonical_cctld(host: &str) -> Option<String> {
    let (rest, tld) = host.rsplit_once('.')?;
    let is_cc = tld.len() == 2 && tld.bytes().all(|b| b.is_ascii_lowercase());
    // Auch `.com.xx`-Varianten und generische Endungen wie `.cat`/`.info` bei Marken erlauben.
    let rest = if is_cc {
        match rest.rsplit_once('.') {
            Some((r, sl)) if SECOND_LEVEL.contains(&sl) => r,
            _ => rest,
        }
    } else if tld == "com" {
        return None;
    } else {
        rest
    };
    let brand = rest.rsplit('.').next()?;
    CCTLD_BRANDS.contains(&brand).then(|| format!("{rest}.com"))
}

fn lookup_host(host: &str) -> Option<(&'static str, Option<SourceCategory>)> {
    let h = normalize_host(host);
    if h.is_empty() {
        return None;
    }
    lookup_suffix(&h).or_else(|| canonical_cctld(&h).and_then(|c| lookup_suffix(&c)))
}

/// Bekannte Quelle fuer einen Hostnamen (`google.de` → `Google`, `t.co` → `Twitter`).
pub fn source_for_host(host: &str) -> Option<&'static str> {
    lookup_host(host).map(|(name, _)| name)
}

/// Hostname aus einem Parameterwert, der wie eine Domain oder URL aussieht.
fn host_like(value: &str) -> Option<&str> {
    let v = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .unwrap_or(value);
    let host = v.split(['/', '?', '#']).next()?;
    let host = host.split(':').next()?;
    let ok = host.contains('.')
        && !host.starts_with('.')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    ok.then_some(host)
}

/// Bekannte Quelle fuer einen (beliebig geschriebenen) Namen, eine Kurzform oder einen Hostnamen.
fn known_source(value: &str) -> Option<(&'static str, Option<SourceCategory>)> {
    let l = value.trim().to_lowercase();
    if l.is_empty() {
        return None;
    }
    if let Some(v) = NAMES.get(&l) {
        return Some(*v);
    }
    if let Some(name) = ALIAS_MAP.get(l.as_str()) {
        return NAMES.get(&name.to_lowercase()).copied().or(Some((*name, None)));
    }
    if let Some((_, name)) = ANDROID_APPS.iter().find(|(p, _)| p.eq_ignore_ascii_case(&l)) {
        return NAMES.get(&name.to_lowercase()).copied().or(Some((*name, None)));
    }
    host_like(&l).and_then(lookup_host)
}

/// Quelle aus `utm_source`, `source` oder `ref` vereinheitlichen (`google` → `Google`,
/// `fb` → `Facebook`, `news.ycombinator.com` → `Hacker News`); Unbekanntes bleibt wie es ist
/// (getrimmt, hoechstens 300 Bytes).
pub fn source_from_param(value: &str) -> String {
    let v = cut(value.trim(), MAX_PARAM);
    if v.is_empty() {
        return String::new();
    }
    match known_source(v) {
        Some((name, _)) => name.to_string(),
        None => v.to_string(),
    }
}

/// Kategorie einer Quelle (Name wie `Google`, Kurzform wie `fb` oder Hostname).
pub fn category_for_source(source: &str) -> Option<SourceCategory> {
    known_source(cut(source, MAX_PARAM)).and_then(|(_, cat)| cat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(r: &str, page: &str) -> Option<String> {
        parse(r, page).map(|r| r.source)
    }

    #[test]
    fn tabelle_gross_genug_und_eindeutig() {
        assert!(HOSTS.len() >= 200, "nur {} Hosts", HOSTS.len());
        let mut seen = std::collections::HashSet::new();
        for (_, _, hosts) in SOURCES {
            for h in *hosts {
                assert!(seen.insert(*h), "doppelt: {h}");
                assert_eq!(*h, h.to_ascii_lowercase());
            }
        }
    }

    #[test]
    fn parse_ungueltig_und_intern() {
        for r in [
            "",
            "  ",
            "null",
            "undefined",
            "kein-url",
            "ftp://example.org/x",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://",
        ] {
            assert_eq!(parse(r, "example.com"), None, "{r}");
        }
        assert_eq!(parse("https://example.com/a", "example.com"), None);
        assert_eq!(parse("https://www.example.com/a", "example.com"), None);
        assert_eq!(parse("https://example.com/a", "www.example.com"), None);
        assert_eq!(parse("https://EXAMPLE.com./a", "example.com"), None);
        // Subdomain ist extern.
        let r = parse("https://blog.example.com/x", "example.com").unwrap();
        assert_eq!(
            (r.host.as_str(), r.source.as_str()),
            ("blog.example.com", "blog.example.com")
        );
    }

    #[test]
    fn parse_bereinigt_url() {
        let r = parse("https://www.Google.de/search?q=geheim#frag", "example.com").unwrap();
        assert_eq!(r.url, "https://www.google.de/search");
        assert_eq!(r.host, "google.de");
        assert_eq!(r.source, "Google");
        let r = parse("http://localhost:8080/pfad?x=1", "example.com").unwrap();
        assert_eq!(r.url, "http://localhost:8080/pfad");
        let r = parse("https://news.ycombinator.com/item?id=1", "example.com").unwrap();
        assert_eq!(r.url, "https://news.ycombinator.com/item");
        assert_eq!(r.source, "Hacker News");
        let lang = format!("https://example.org/{}", "a".repeat(5000));
        assert_eq!(parse(&lang, "example.com").unwrap().url.len(), 2000);
    }

    #[test]
    fn android_apps() {
        let r = parse("android-app://com.google.android.gm", "example.com").unwrap();
        assert_eq!(r.source, "Gmail");
        assert_eq!(r.url, "android-app://com.google.android.gm");
        assert_eq!(r.host, "com.google.android.gm");
        assert_eq!(
            src(
                "android-app://com.google.android.googlequicksearchbox/https/www.google.com",
                "example.com"
            )
            .as_deref(),
            Some("Google")
        );
        assert_eq!(
            src("android-app://org.example.app", "example.com").as_deref(),
            Some("org.example.app")
        );
    }

    #[test]
    fn suchmaschinen() {
        for (h, s) in [
            ("google.com", "Google"),
            ("google.de", "Google"),
            ("google.ch", "Google"),
            ("google.co.uk", "Google"),
            ("google.com.br", "Google"),
            ("google.co.jp", "Google"),
            ("www.google.com.au", "Google"),
            ("bing.com", "Bing"),
            ("cn.bing.com", "Bing"),
            ("duckduckgo.com", "DuckDuckGo"),
            ("html.duckduckgo.com", "DuckDuckGo"),
            ("search.yahoo.com", "Yahoo!"),
            ("de.search.yahoo.com", "Yahoo!"),
            ("search.yahoo.co.jp", "Yahoo!"),
            ("yandex.ru", "Yandex"),
            ("yandex.com.tr", "Yandex"),
            ("ya.ru", "Yandex"),
            ("baidu.com", "Baidu"),
            ("ecosia.org", "Ecosia"),
            ("qwant.com", "Qwant"),
            ("startpage.com", "Startpage"),
            ("search.brave.com", "Brave Search"),
            ("kagi.com", "Kagi"),
            ("search.aol.com", "AOL"),
            ("search.naver.com", "Naver"),
            ("seznam.cz", "Seznam"),
            ("swisscows.com", "Swisscows"),
            ("mojeek.com", "Mojeek"),
            ("perplexity.ai", "Perplexity"),
            ("chatgpt.com", "ChatGPT"),
            ("chat.openai.com", "ChatGPT"),
            ("claude.ai", "Claude"),
            ("gemini.google.com", "Gemini"),
            ("copilot.microsoft.com", "Copilot"),
            ("you.com", "You.com"),
            ("phind.com", "Phind"),
        ] {
            assert_eq!(source_for_host(h), Some(s), "{h}");
        }
    }

    #[test]
    fn soziale_netzwerke_video_mail() {
        for (h, s) in [
            ("facebook.com", "Facebook"),
            ("l.facebook.com", "Facebook"),
            ("m.facebook.com", "Facebook"),
            ("lm.facebook.com", "Facebook"),
            ("instagram.com", "Instagram"),
            ("l.instagram.com", "Instagram"),
            ("t.co", "Twitter"),
            ("twitter.com", "Twitter"),
            ("x.com", "Twitter"),
            ("linkedin.com", "LinkedIn"),
            ("lnkd.in", "LinkedIn"),
            ("de.linkedin.com", "LinkedIn"),
            ("old.reddit.com", "Reddit"),
            ("out.reddit.com", "Reddit"),
            ("pinterest.de", "Pinterest"),
            ("tiktok.com", "TikTok"),
            ("threads.net", "Threads"),
            ("mastodon.social", "Mastodon"),
            ("bsky.app", "Bluesky"),
            ("news.ycombinator.com", "Hacker News"),
            ("producthunt.com", "Product Hunt"),
            ("lobste.rs", "Lobsters"),
            ("t.me", "Telegram"),
            ("youtube.com", "YouTube"),
            ("m.youtube.com", "YouTube"),
            ("youtu.be", "YouTube"),
            ("vimeo.com", "Vimeo"),
            ("twitch.tv", "Twitch"),
            ("mail.google.com", "Gmail"),
            ("outlook.live.com", "Outlook"),
            ("outlook.office.com", "Outlook"),
            ("mail.yahoo.com", "Yahoo! Mail"),
            ("navigator.gmx.net", "GMX"),
            ("web.de", "Web.de"),
            ("mail.proton.me", "Proton Mail"),
            ("github.com", "GitHub"),
            ("de.wikipedia.org", "Wikipedia"),
            ("en.m.wikipedia.org", "Wikipedia"),
            ("news.google.com", "Google News"),
            ("news.google.de", "Google News"),
            ("amazon.de", "Amazon"),
            ("amazon.co.uk", "Amazon"),
            ("ebay.de", "eBay"),
            ("etsy.com", "Etsy"),
        ] {
            assert_eq!(source_for_host(h), Some(s), "{h}");
        }
    }

    #[test]
    fn unbekannte_hosts() {
        for h in [
            "example.com",
            "notgoogle.com",
            "google.example.com",
            "evilyahoo.com",
            "com",
            "",
            "de",
        ] {
            assert_eq!(source_for_host(h), None, "{h}");
        }
    }

    #[test]
    fn parameter() {
        for (v, s) in [
            ("google", "Google"),
            ("Google", "Google"),
            ("GOOGLE", "Google"),
            ("fb", "Facebook"),
            ("facebook", "Facebook"),
            ("ig", "Instagram"),
            ("instagram", "Instagram"),
            ("tw", "Twitter"),
            ("twitter", "Twitter"),
            ("x", "Twitter"),
            ("li", "LinkedIn"),
            ("linkedin", "LinkedIn"),
            ("yt", "YouTube"),
            ("youtube", "YouTube"),
            ("reddit", "Reddit"),
            ("duckduckgo", "DuckDuckGo"),
            ("bing", "Bing"),
            ("news.ycombinator.com", "Hacker News"),
            ("https://www.reddit.com/r/rust", "Reddit"),
            ("chatgpt.com", "ChatGPT"),
            ("  newsletter ", "newsletter"),
            ("Herbst-Aktion", "Herbst-Aktion"),
            ("", ""),
        ] {
            assert_eq!(source_from_param(v), s, "{v}");
        }
        assert_eq!(source_from_param(&"ä".repeat(400)).len(), 300);
    }

    #[test]
    fn kategorien() {
        assert_eq!(category_for_source("Google"), Some(SourceCategory::Search));
        assert_eq!(category_for_source("google"), Some(SourceCategory::Search));
        assert_eq!(category_for_source("Facebook"), Some(SourceCategory::Social));
        assert_eq!(category_for_source("fb"), Some(SourceCategory::Social));
        assert_eq!(category_for_source("YouTube"), Some(SourceCategory::Video));
        assert_eq!(category_for_source("Amazon"), Some(SourceCategory::Shopping));
        assert_eq!(category_for_source("Gmail"), Some(SourceCategory::Email));
        assert_eq!(
            category_for_source("mail.google.com"),
            Some(SourceCategory::Email)
        );
        assert_eq!(category_for_source("Hacker News"), Some(SourceCategory::Social));
        assert_eq!(category_for_source("GitHub"), None);
        assert_eq!(category_for_source("example.com"), None);
        assert_eq!(category_for_source(""), None);
    }
}
