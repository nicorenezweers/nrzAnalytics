//! Kanal (Acquisition Channel) wie bei Plausible/GA4 bestimmen.
//!
//! Nachbau der Reihenfolge aus Plausibles `Plausible.Ingestion.Acquisition` (angelehnt an das
//! GA4 Default Channel Grouping). Quellen-Kategorien kommen aus [`super::referrer`].

use super::referrer::{SourceCategory, category_for_source};

/// Eingaben fuer die Kanal-Bestimmung (alle Werte wie gespeichert, leer = fehlt).
#[derive(Clone, Copy, Debug, Default)]
pub struct ChannelInput<'a> {
    /// Quelle, z. B. `Google`, `Facebook`, `news.ycombinator.com`; leer = direkt.
    pub source: &'a str,
    /// Hostname des Verweises (ohne `www.`), leer wenn keiner.
    pub referrer_host: &'a str,
    pub utm_medium: &'a str,
    pub utm_source: &'a str,
    pub utm_campaign: &'a str,
    /// `gclid`/`gbraid`/`wbraid` in der URL (Google Ads).
    pub google_click_id: bool,
    /// `msclkid` in der URL (Microsoft Ads).
    pub microsoft_click_id: bool,
}

/// Kanal: `Direct`, `Organic Search`, `Paid Search`, `Organic Social`, `Paid Social`,
/// `Organic Video`, `Paid Video`, `Organic Shopping`, `Paid Shopping`, `Email`, `Affiliates`,
/// `Display`, `Paid Other`, `Referral`, `Cross-network`, `SMS`, `Audio` oder
/// `Mobile Push Notifications`.
pub fn classify(input: &ChannelInput<'_>) -> &'static str {
    let source = input.source.trim().to_lowercase();
    let medium = input.utm_medium.trim().to_lowercase();
    let campaign = input.utm_campaign.trim().to_lowercase();
    let utm_source = input.utm_source.trim().to_lowercase();
    let category = if source.is_empty() {
        None
    } else {
        category_for_source(&source)
    };
    let is = |c: SourceCategory| category == Some(c);
    let paid = paid_medium(&medium);
    let shopping_campaign = shop_match(&campaign);
    let shopping_source = is(SourceCategory::Shopping) || shop_match(&source);

    if campaign.contains("cross-network") {
        return "Cross-network";
    }
    if (shopping_source || shopping_campaign) && paid {
        return "Paid Shopping";
    }
    if (is(SourceCategory::Search) && paid)
        || (source == "google" && input.google_click_id)
        || (source == "bing" && input.microsoft_click_id)
    {
        return "Paid Search";
    }
    if is(SourceCategory::Social) && paid {
        return "Paid Social";
    }
    if is(SourceCategory::Video) && paid {
        return "Paid Video";
    }
    if matches!(
        medium.as_str(),
        "display" | "banner" | "expandable" | "interstitial" | "cpm"
    ) {
        return "Display";
    }
    if paid {
        return "Paid Other";
    }
    if shopping_source || shopping_campaign {
        return "Organic Shopping";
    }
    if is(SourceCategory::Social)
        || matches!(
            medium.as_str(),
            "social" | "social-network" | "social-media" | "sm" | "social network" | "social media"
        )
    {
        return "Organic Social";
    }
    if is(SourceCategory::Video) || medium.contains("video") {
        return "Organic Video";
    }
    if is(SourceCategory::Search) || medium == "organic" {
        return "Organic Search";
    }
    if is(SourceCategory::Email) || is_email_tag(&source) || is_email_tag(&medium) {
        return "Email";
    }
    if medium == "affiliate" {
        return "Affiliates";
    }
    if medium == "audio" {
        return "Audio";
    }
    if utm_source == "sms" || medium == "sms" {
        return "SMS";
    }
    if medium.ends_with("push")
        || medium.contains("mobile")
        || medium.contains("notification")
        || source == "firebase"
    {
        return "Mobile Push Notifications";
    }
    if matches!(medium.as_str(), "referral" | "app" | "link")
        || !source.is_empty()
        || !input.referrer_host.trim().is_empty()
    {
        return "Referral";
    }
    "Direct"
}

/// Bezahltes Medium: `^(.*cp.*|ppc|retargeting|paid.*)$`.
fn paid_medium(medium: &str) -> bool {
    medium.contains("cp") || medium == "ppc" || medium == "retargeting" || medium.starts_with("paid")
}

/// Shopping-Muster `^(.*(([^a-df-z]|^)shop|shopping).*)$`: `shop` am Anfang oder nach einem
/// Zeichen ausser a–d/f–z (also auch `eshop`, aber nicht `workshop`), oder `shopping`.
fn shop_match(s: &str) -> bool {
    if s.contains("shopping") {
        return true;
    }
    let b = s.as_bytes();
    s.match_indices("shop").any(|(i, _)| {
        i == 0 || {
            let c = b[i - 1];
            !(c.is_ascii_lowercase() && c != b'e')
        }
    })
}

/// E-Mail-Kennzeichen in Quelle oder Medium.
fn is_email_tag(s: &str) -> bool {
    matches!(s, "email" | "e-mail" | "e_mail" | "e mail" | "newsletter")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(source: &str, medium: &str, campaign: &str) -> &'static str {
        classify(&ChannelInput {
            source,
            utm_medium: medium,
            utm_source: source,
            utm_campaign: campaign,
            ..ChannelInput::default()
        })
    }

    #[test]
    fn direkt_und_verweis() {
        assert_eq!(ch("", "", ""), "Direct");
        assert_eq!(ch("example.com", "", ""), "Referral");
        assert_eq!(ch("GitHub", "", ""), "Referral");
        assert_eq!(ch("", "referral", ""), "Referral");
        assert_eq!(ch("", "link", ""), "Referral");
        let i = ChannelInput {
            referrer_host: "example.org",
            ..ChannelInput::default()
        };
        assert_eq!(classify(&i), "Referral");
    }

    #[test]
    fn suche() {
        assert_eq!(ch("Google", "", ""), "Organic Search");
        assert_eq!(ch("google", "", ""), "Organic Search");
        assert_eq!(ch("Bing", "", ""), "Organic Search");
        assert_eq!(ch("DuckDuckGo", "", ""), "Organic Search");
        assert_eq!(ch("Ecosia", "", ""), "Organic Search");
        assert_eq!(ch("example.com", "organic", ""), "Organic Search");
        assert_eq!(ch("Google", "cpc", ""), "Paid Search");
        assert_eq!(ch("Bing", "ppc", ""), "Paid Search");
        assert_eq!(ch("Google", "paid_search", ""), "Paid Search");
        let g = ChannelInput {
            source: "Google",
            google_click_id: true,
            ..ChannelInput::default()
        };
        assert_eq!(classify(&g), "Paid Search");
        let b = ChannelInput {
            source: "Bing",
            microsoft_click_id: true,
            ..ChannelInput::default()
        };
        assert_eq!(classify(&b), "Paid Search");
        // Click-ID passt nicht zur Quelle.
        let x = ChannelInput {
            source: "Bing",
            google_click_id: true,
            ..ChannelInput::default()
        };
        assert_eq!(classify(&x), "Organic Search");
    }

    #[test]
    fn sozial_und_video() {
        assert_eq!(ch("Facebook", "", ""), "Organic Social");
        assert_eq!(ch("Twitter", "", ""), "Organic Social");
        assert_eq!(ch("Hacker News", "", ""), "Organic Social");
        assert_eq!(ch("Facebook", "cpc", ""), "Paid Social");
        assert_eq!(ch("Instagram", "paid", ""), "Paid Social");
        assert_eq!(ch("irgendwas", "social", ""), "Organic Social");
        assert_eq!(ch("irgendwas", "social media", ""), "Organic Social");
        assert_eq!(ch("YouTube", "", ""), "Organic Video");
        assert_eq!(ch("YouTube", "cpv", ""), "Paid Video");
        assert_eq!(ch("irgendwas", "video", ""), "Organic Video");
    }

    #[test]
    fn shopping() {
        assert_eq!(ch("Amazon", "", ""), "Organic Shopping");
        assert_eq!(ch("Amazon", "cpc", ""), "Paid Shopping");
        assert_eq!(ch("foo", "", "summer-shop"), "Organic Shopping");
        assert_eq!(ch("foo", "cpc", "shopping-week"), "Paid Shopping");
        assert_eq!(ch("myeshop", "", ""), "Organic Shopping");
        assert_eq!(ch("workshop", "", ""), "Referral");
    }

    #[test]
    fn werbung() {
        assert_eq!(ch("foo", "", "q4-cross-network"), "Cross-network");
        assert_eq!(ch("foo", "display", ""), "Display");
        assert_eq!(ch("foo", "banner", ""), "Display");
        assert_eq!(ch("foo", "cpm", ""), "Display");
        assert_eq!(ch("foo", "retargeting", ""), "Paid Other");
        assert_eq!(ch("foo", "paid", ""), "Paid Other");
    }

    #[test]
    fn email_und_rest() {
        assert_eq!(ch("newsletter", "", ""), "Email");
        assert_eq!(ch("foo", "email", ""), "Email");
        assert_eq!(ch("foo", "E-Mail", ""), "Email");
        assert_eq!(ch("Gmail", "", ""), "Email");
        assert_eq!(ch("Outlook", "", ""), "Email");
        assert_eq!(ch("foo", "affiliate", ""), "Affiliates");
        assert_eq!(ch("foo", "audio", ""), "Audio");
        assert_eq!(ch("sms", "", ""), "SMS");
        assert_eq!(ch("foo", "sms", ""), "SMS");
        assert_eq!(ch("foo", "web-push", ""), "Mobile Push Notifications");
        assert_eq!(ch("foo", "mobile", ""), "Mobile Push Notifications");
        assert_eq!(ch("foo", "notification", ""), "Mobile Push Notifications");
        assert_eq!(ch("firebase", "", ""), "Mobile Push Notifications");
    }

    #[test]
    fn muster() {
        assert!(paid_medium("cpc"));
        assert!(paid_medium("ppc"));
        assert!(paid_medium("paidsocial"));
        assert!(!paid_medium("organic"));
        assert!(shop_match("shop"));
        assert!(shop_match("e-shop"));
        assert!(shop_match("eshop"));
        assert!(!shop_match("workshop"));
        assert!(shop_match("workshopping"));
    }
}
