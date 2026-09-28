//! **A link preview is not somebody at a bedside.**
//!
//! When a patient's link is pasted into Facebook, LINE, Slack, X or Discord, the platform fetches
//! the page itself to draw a card under the link — no person, no script run, often several times
//! from several machines. On 26 Sep 2026 `facebookexternalhit` fetched one patient page nine times
//! in six minutes, and `bedsides_opened` counted nine people. A headless browser is ours (the
//! capture rig), and is not a visitor either. Neither is counted; every ordinary browser still is.

use vitals_web::usage::is_not_a_person;

#[test]
fn a_link_preview_or_a_headless_browser_is_not_counted_as_a_person() {
    for bot in [
        "facebookexternalhit/1.1 (+http://www.facebook.com/externalhit_uatext.php)",
        "facebookcatalog/1.0",
        "Mozilla/5.0 (compatible; Line/13.1.0; +https://line.me)",
        "Slackbot-LinkExpanding 1.0 (+https://api.slack.com/robots)",
        "Twitterbot/1.0",
        "Mozilla/5.0 (compatible; Discordbot/2.0; +https://discordapp.com)",
        "TelegramBot (like TwitterBot)",
        "WhatsApp/2.23.20.0",
        "LinkedInBot/1.0 (compatible; Mozilla/5.0; Apache-HttpClient +http://www.linkedin.com)",
        "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/131.0.0.0 Safari/537.36",
        "",
    ] {
        assert!(is_not_a_person(bot), "counted as a person: {bot:?}");
    }
    for person in [
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36",
        // Facebook's and LINE's in-app browsers are people who tapped the link, not the preview bot.
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 [FBAN/FBIOS;FBAV/480.0.0.40.108]",
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/14.15.0",
    ] {
        assert!(!is_not_a_person(person), "a person was not counted: {person:?}");
    }
}
