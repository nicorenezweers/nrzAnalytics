//! Bots und Verweis-Spam erkennen.
//!
//! Die Bot-Erkennung arbeitet mit einem einzigen, einmalig gebauten regulaeren Ausdruck ueber den
//! klein geschriebenen User-Agent. Woerter wie `bot` werden nur am Wortende erkannt, damit echte
//! Geraete wie `CUBOT`-Handys oder Woerter wie `bottom` nicht faelschlich als Bot gelten.

use std::sync::LazyLock;

use regex::Regex;

/// Muster fuer Crawler, Vorschau-Dienste, Headless-Browser, Monitoring und HTTP-Bibliotheken.
static BOT_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let patterns: &[&str] = &[
        // Allgemeine Kennzeichen (Wortende beachten: "googlebot/", aber nicht "bottom").
        r"bots?\b",
        r"crawl",
        r"spider",
        r"scraper",
        r"slurp",
        r"\+https?://",
        // Suchmaschinen- und Werbe-Crawler
        r"mediapartners",
        r"adsbot",
        r"bingpreview",
        r"feedfetcher",
        r"apis-google",
        r"google-extended",
        r"googleother",
        r"google-inspectiontool",
        r"google-read-aloud",
        r"google web preview",
        r"google-safety",
        r"yandex(?:images|metrika|favicons|turbo|video|media|direct|webmaster|pagechecker|screenshot|news|market|calendar|blogs|sitelinks|fordomain|searchshop|adnet|antivirus|partner|rca|tracker|verticals|vertis|accessibility|renderresources|userproxy|dialogs|additional|catalog|account|ontodb)",
        r"ia_archiver",
        r"archive\.org",
        r"diffbot",
        // Link-Vorschauen sozialer Netzwerke und Messenger
        r"facebookexternalhit",
        r"facebookcatalog",
        r"meta-external",
        r"embedly",
        r"quora link preview",
        r"outbrain",
        r"pinterest/\d",
        r"^whatsapp/",
        r"skypeuripreview",
        r"url ?preview",
        r"link ?preview",
        r"iframely",
        r"vkshare",
        r"redditbot",
        r"snap url preview",
        r"mastodon/",
        r"pleroma",
        r"akkoma",
        r"misskey",
        // KI- und SEO-Crawler
        r"gptbot",
        r"chatgpt-user",
        r"oai-searchbot",
        r"claude-web",
        r"claude-user",
        r"claude-searchbot",
        r"anthropic-ai",
        r"perplexity",
        r"ccbot",
        r"bytespider",
        r"cohere-ai",
        r"semrush",
        r"ahrefs",
        r"seokicks",
        r"sistrix",
        r"screaming frog",
        r"serpstat",
        r"dataforseo",
        r"scrapy",
        r"newspaper/",
        r"feedly",
        r"inoreader",
        r"newsblur",
        r"tiny tiny rss",
        r"feedburner",
        r"\brss",
        r"validator",
        // Headless-Browser und Automatisierung
        r"headless",
        r"phantomjs",
        r"puppeteer",
        r"playwright",
        r"selenium",
        r"webdriver",
        r"cypress",
        r"jsdom",
        r"lighthouse",
        r"page ?speed",
        r"gtmetrix",
        r"pingdom",
        r"uptimerobot",
        r"statuscake",
        r"site24x7",
        r"better ?uptime",
        r"uptime[- ]kuma",
        r"monitoring",
        r"synthetics",
        r"newrelicpinger",
        r"checkly",
        r"hetrixtools",
        r"freshping",
        r"nagios",
        r"zabbix",
        r"prometheus",
        r"blackbox",
        r"zgrab",
        r"masscan",
        r"nmap",
        r"nikto",
        r"sqlmap",
        r"censys",
        r"netcraft",
        r"researchscan",
        r"internetmeasurement",
        // HTTP-Bibliotheken und Werkzeuge
        r"curl",
        r"wget",
        r"python",
        r"aiohttp",
        r"httpx",
        r"go-http-client",
        r"okhttp",
        r"\bjava/",
        r"apache-httpclient",
        r"jakarta",
        r"axios",
        r"node-fetch",
        r"\bnode/",
        r"undici",
        r"\bdeno/",
        r"\bbun/",
        r"libwww-perl",
        r"lwp::",
        r"mechanize",
        r"guzzlehttp",
        r"\bphp/",
        r"symfony",
        r"reqwest",
        r"hyper/",
        r"insomnia",
        r"postman",
        r"httpie",
        r"\bruby\b",
        r"faraday",
        r"rest-client",
        r"winhttp",
        r"powershell",
        r"httpclient",
        r"fasthttp",
        r"colly",
        r"nutch",
        r"heritrix",
        r"dart:io",
    ];
    // Die Muster sind fest und gueltig; scheitert der Bau trotzdem, wird nichts als Bot gewertet
    // (keine Panik zur Laufzeit).
    Regex::new(&patterns.join("|")).ok()
});

/// `true` fuer Crawler, Bots, Headless-Browser, Monitoring-Dienste und HTTP-Bibliotheken
/// (curl, python-requests, Go-http-client …) sowie leere oder sehr kurze User-Agents.
pub fn is_bot_user_agent(ua: &str) -> bool {
    let ua = ua.trim();
    if ua.len() < 10 {
        return true;
    }
    let mut l = ua.to_ascii_lowercase();
    // Handy-Hersteller "Cubot" ist kein Bot.
    if l.contains("cubot") {
        l = l.replace("cubot", "cu-phone");
    }
    BOT_RE.as_ref().is_some_and(|re| re.is_match(&l))
}

/// Bekannte Verweis-Spam-Domains (Auszug aus den gaengigen Referrer-Spam-Listen).
const SPAM_DOMAINS: &[&str] = &[
    "semalt.com",
    "semalt.semalt.com",
    "buttons-for-website.com",
    "buttons-for-your-website.com",
    "darodar.com",
    "best-seo-offer.com",
    "best-seo-solution.com",
    "7makemoneyonline.com",
    "ilovevitaly.com",
    "ilovevitaly.ru",
    "ilovevitaly.co",
    "priceg.com",
    "hulfingtonpost.com",
    "blackhatworth.com",
    "econom.co",
    "savetubevideo.com",
    "kambasoft.com",
    "lumb.co",
    "o-o-6-o-o.com",
    "o-o-8-o-o.com",
    "cenoval.ru",
    "simple-share-buttons.com",
    "social-buttons.com",
    "free-social-buttons.com",
    "free-share-buttons.com",
    "get-free-traffic-now.com",
    "free-traffic.xyz",
    "traffic2money.com",
    "trafficmonetize.org",
    "trafficmonetizer.org",
    "webmonetizer.net",
    "4webmasters.org",
    "100dollars-seo.com",
    "best-seo-software.xyz",
    "seo-platform.com",
    "seoexperimenty.ru",
    "floating-share-buttons.com",
    "event-tracking.com",
    "fix-website-errors.com",
    "site-auditor.online",
    "website-analyzer.info",
    "keywords-monitoring-your-success.com",
    "googlsucks.com",
    "anticrawler.org",
    "rankingchart.de",
    "ranksonic.info",
    "ranksonic.org",
    "videos-for-your-business.com",
    "success-seo.com",
    "copyrightclaims.org",
    "sexyali.com",
    "domination.ml",
    "bestwebsitesawards.com",
    "torture.ml",
    "humanorightswatch.org",
    "share-buttons.xyz",
    "erot.co",
    "screentoolkit.com",
    "makemoneyonline.com",
    "trafficbot.life",
    "aliexpress-seo.ru",
    "descargar-musica-gratis.net",
    "novosti-hi-tech.ru",
    "pornhub-forum.ga",
    "rank-checker.online",
    "mycheaptraffic.com",
    "seo-traffic.xyz",
    "monetizationking.net",
    "qualitymarketzone.com",
    "fbdownloader.com",
    "backlinks-fast-top.com",
];

/// `true` fuer bekannte Verweis-Spam-Domains (Hostname ohne `www.`), inklusive Subdomains.
pub fn is_spam_referrer(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return false;
    }
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    SPAM_DOMAINS.iter().any(|d| {
        host == *d
            || (host.len() > d.len()
                && host.ends_with(d)
                && host.as_bytes()[host.len() - d.len() - 1] == b'.')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leer_und_kurz_sind_bots() {
        assert!(is_bot_user_agent(""));
        assert!(is_bot_user_agent("   "));
        assert!(is_bot_user_agent("Mozilla"));
    }

    #[test]
    fn crawler() {
        for ua in [
            "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
            "Mozilla/5.0 (Linux; Android 6.0.1; Nexus 5X Build/MMB29P) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.6778.69 Mobile Safari/537.36 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
            "Mozilla/5.0 (compatible; bingbot/2.0; +http://www.bing.com/bingbot.htm)",
            "Mediapartners-Google",
            "AdsBot-Google (+http://www.google.com/adsbot.html)",
            "facebookexternalhit/1.1 (+http://www.facebook.com/externalhit_uatext.php)",
            "Twitterbot/1.0",
            "LinkedInBot/1.0 (compatible; Mozilla/5.0; Apache-HttpClient +http://www.linkedin.com)",
            "Slackbot-LinkExpanding 1.0 (+https://api.slack.com/robots)",
            "Mozilla/5.0 (compatible; Discordbot/2.0; +https://discordapp.com)",
            "TelegramBot (like TwitterBot)",
            "WhatsApp/2.23.20.0 A",
            "Pinterest/0.2 (+https://www.pinterest.com/bot.html)",
            "Mozilla/5.0 (compatible; YandexBot/3.0; +http://yandex.com/bots)",
            "Mozilla/5.0 (compatible; YandexImages/3.0; +http://yandex.com/bots)",
            "Mozilla/5.0 (compatible; Baiduspider/2.0; +http://www.baidu.com/search/spider.html)",
            "DuckDuckBot/1.1; (+http://duckduckgo.com/duckduckbot.html)",
            "Mozilla/5.0 (Linux; Android 7.0;) AppleWebKit/537.36 (KHTML, like Gecko) Mobile Safari/537.36 (compatible; PetalBot;+https://webmaster.petalsearch.com/site/petalbot)",
            "Mozilla/5.0 (compatible; SemrushBot/7~bl; +http://www.semrush.com/bot.html)",
            "Mozilla/5.0 (compatible; AhrefsBot/7.0; +http://ahrefs.com/robot/)",
            "Mozilla/5.0 (compatible; MJ12bot/v1.4.8; http://mj12bot.com/)",
            "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; GPTBot/1.2; +https://openai.com/gptbot)",
            "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko); compatible; ChatGPT-User/1.0; +https://openai.com/bot",
            "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ClaudeBot/1.0; +claudebot@anthropic.com)",
            "anthropic-ai/1.0 (compatible)",
            "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; PerplexityBot/1.0; +https://perplexity.ai/perplexitybot)",
            "CCBot/2.0 (https://commoncrawl.org/faq/)",
            "Mozilla/5.0 (Linux; Android 5.0) AppleWebKit/537.36 (KHTML, like Gecko) Mobile Safari/537.36 (compatible; Bytespider; spider-feedback@bytedance.com)",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.5 Safari/605.1.15 (Applebot/0.1; +http://www.apple.com/go/applebot)",
            "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; Amazonbot/0.1; +https://developer.amazon.com/support/amazonbot)",
            "cohere-ai",
        ] {
            assert!(is_bot_user_agent(ua), "{ua}");
        }
    }

    #[test]
    fn headless_und_monitoring() {
        for ua in [
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/131.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Unknown; Linux x86_64) AppleWebKit/538.1 (KHTML, like Gecko) PhantomJS/2.1.1 Safari/538.1",
            "Mozilla/5.0 (Linux; Android 11; moto g power (2022)) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36 Chrome-Lighthouse",
            "Mozilla/5.0 (compatible; Google Page Speed Insights)",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Cypress/13.6.0",
            "Mozilla/5.0 (compatible; UptimeRobot/2.0; http://www.uptimerobot.com/)",
            "Pingdom.com_bot_version_1.4_(http://www.pingdom.com/)",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; StatusCake)",
            "Mozilla/5.0 (compatible; Site24x7)",
            "Better Uptime Bot Mozilla/5.0",
            "Uptime-Kuma/1.23.11",
            "Mozilla/5.0 (X11; Linux x86_64) GTmetrix",
        ] {
            assert!(is_bot_user_agent(ua), "{ua}");
        }
    }

    #[test]
    fn http_bibliotheken() {
        for ua in [
            "curl/8.7.1",
            "Wget/1.21.4",
            "python-requests/2.32.3",
            "Python-urllib/3.12",
            "Python/3.12 aiohttp/3.9.5",
            "python-httpx/0.27.0",
            "Go-http-client/2.0",
            "okhttp/4.12.0",
            "Java/17.0.2",
            "Apache-HttpClient/4.5.14 (Java/17.0.2)",
            "axios/1.7.2",
            "node-fetch/1.0 (+https://github.com/bitinn/node-fetch)",
            "undici",
            "libwww-perl/6.72",
            "GuzzleHttp/7",
            "reqwest/0.12",
            "insomnia/2023.5.8",
            "PostmanRuntime/7.39.0",
        ] {
            assert!(is_bot_user_agent(ua), "{ua}");
        }
    }

    #[test]
    fn echte_browser_sind_keine_bots() {
        for ua in [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
            "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0",
            "Mozilla/5.0 (Linux; Android 10; CUBOT_X30) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.6778.81 Mobile Safari/537.36",
            "Mozilla/5.0 (Linux; Android 12; CUBOT KINGKONG 7 Build/SP1A.210812.016) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.6723.107 Mobile Safari/537.36",
            "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Instagram 352.0.0.29.88 (iPhone15,3; iOS 17_6; de_DE; de; scale=3.00; 1290x2796; 646588129)",
            "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP2A.240905.003; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/130.0.6723.102 Mobile Safari/537.36 [Pinterest/Android]",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
        ] {
            assert!(!is_bot_user_agent(ua), "{ua}");
        }
    }

    #[test]
    fn spam_referrer() {
        assert!(is_spam_referrer("semalt.com"));
        assert!(is_spam_referrer("www.semalt.com"));
        assert!(is_spam_referrer("abc.semalt.com"));
        assert!(is_spam_referrer("Darodar.com"));
        assert!(is_spam_referrer("forum.buttons-for-website.com"));
        assert!(is_spam_referrer("hulfingtonpost.com"));
        assert!(!is_spam_referrer("notsemalt.com"));
        assert!(!is_spam_referrer("google.com"));
        assert!(!is_spam_referrer("huffingtonpost.com"));
        assert!(!is_spam_referrer(""));
    }
}
