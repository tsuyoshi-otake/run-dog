//! Privacy pages must match the implementation notes and stay off Google Fonts.

const PRIVACY: &str = include_str!("../docs/PRIVACY.md");
const INDEX: &str = include_str!("../docs/index.html");
const PAGE: &str = include_str!("../docs/privacy.html");
const APP: &str = include_str!("../docs/app.js");
const CSS: &str = include_str!("../docs/style.css");
const I18N: &str = include_str!("../docs/i18n.js");

#[test]
fn pages_do_not_load_google_fonts() {
    for (name, source) in [
        ("index.html", INDEX),
        ("privacy.html", PAGE),
        ("app.js", APP),
        ("style.css", CSS),
    ] {
        let lower = source.to_ascii_lowercase();
        assert!(
            !lower.contains("fonts.googleapis.com"),
            "{name} must not load Google Fonts"
        );
        assert!(
            !lower.contains("fonts.gstatic.com"),
            "{name} must not preconnect Google Fonts"
        );
    }
    assert!(
        !APP.contains("function loadFont"),
        "app.js must not inject per-language webfonts"
    );
}

#[test]
fn privacy_document_covers_required_facts() {
    for needle in [
        "api.anthropic.com",
        "chatgpt.com",
        "api.github.com",
        "no RunDog-owned server",
        "raw prompts",
        ".credentials.json",
        "auth.json",
        "ReplaceFileW",
        "5 minutes",
        "60 seconds",
        "analytics",
        "Automatic vendor usage-limit queries",
        "not turned Off",
    ] {
        assert!(
            PRIVACY.contains(needle),
            "docs/PRIVACY.md must mention {needle}"
        );
    }
}

#[test]
fn privacy_pages_state_no_rundog_server_and_no_raw_prompts() {
    assert!(I18N.contains("RunDog 自身のサーバー"));
    assert!(I18N.contains("no RunDog-owned server") || I18N.contains("RunDog-owned server"));
    assert!(I18N.contains("プロンプト") || I18N.contains("raw prompt"));
}
