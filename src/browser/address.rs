use serde::Deserialize;
use std::collections::HashMap;
use url::form_urlencoded;

#[derive(Clone, Debug, Deserialize)]
pub struct SearchConfig {
    pub default_engine: String,
    pub engines: HashMap<String, String>,
}

const DEFAULT_CONFIG: &str = include_str!("../../config/search.toml");

const FALLBACK_ENGINE: &str = "https://search.brave.com/search?q=%s";

/// Known search engines as (id, display label) pairs.
/// Order defines the dropdown order in Settings and Welcome.
pub const KNOWN_ENGINES: &[(&str, &str)] = &[
    ("brave", "Brave Search"),
    ("duckduckgo", "DuckDuckGo"),
    ("google", "Google"),
    ("bing", "Bing"),
    ("yahoo", "Yahoo"),
    ("startpage", "Startpage"),
    ("ecosia", "Ecosia"),
    ("qwant", "Qwant"),
];

fn default_config() -> SearchConfig {
    toml::from_str(DEFAULT_CONFIG).unwrap_or_else(|_| SearchConfig {
        default_engine: "brave".to_string(),
        engines: [("brave".to_string(), FALLBACK_ENGINE.to_string())].into(),
    })
}

/// Display label for an engine id (falls back to the id itself).
pub fn engine_display_name(id: &str) -> &str {
    KNOWN_ENGINES
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, name)| *name)
        .unwrap_or(id)
}

pub fn load_config() -> SearchConfig {
    // The engine list is embedded at build time. (Reading it from a path
    // relative to the working directory made behaviour depend on where the
    // browser was launched from.)
    default_config()
}

pub fn resolve_input(input: &str, config: &SearchConfig) -> String {
    let input = input.trim();

    if input.is_empty() {
        return "axys://newtab".to_string();
    }

    let lower = input.to_ascii_lowercase();

    if lower.starts_with("axys://") {
        return lower;
    }

    if lower == "about:blank" {
        return lower;
    }

    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
    {
        // Normalise only the scheme to lower case: `HTTPS://x` would
        // otherwise be dropped by the navigation guard, which is
        // case-sensitive. The rest of the URL is preserved verbatim
        // (paths are case-sensitive).
        let scheme_len = lower.find("://").map(|i| i + 3).unwrap_or(0);

        return format!("{}{}", &lower[..scheme_len], &input[scheme_len..]);
    }

    if !input.contains(' ')
        && (lower == "localhost"
            || lower.starts_with("localhost:")
            || lower.starts_with("localhost/"))
    {
        return format!("http://{input}");
    }

    // Bare IP literals are local/LAN addresses: default to plain HTTP (like
    // Chromium), not HTTPS, and accept bracketed IPv6 (`[::1]:8080`) which
    // has no dot and would otherwise be sent to the search engine.
    if !input.contains(' ') {
        let host = input.split(['/', '?', '#']).next().unwrap_or(input);

        let host_only = match host.rsplit_once(':') {
            Some((head, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
                head
            }
            _ => host,
        };

        if host_only.starts_with('[') || host_only.parse::<std::net::Ipv4Addr>().is_ok() {
            return format!("http://{input}");
        }
    }

    if input.contains('.') && !input.contains(' ') {
        let host = input.trim_end_matches('/');

        if host.eq_ignore_ascii_case("youtube.com") {
            return "https://www.youtube.com".to_string();
        }

        if host.eq_ignore_ascii_case("www.youtube.com") {
            return "https://www.youtube.com".to_string();
        }

        return format!("https://{input}");
    }

    let engine = config
        .engines
        .get(&config.default_engine)
        .or_else(|| config.engines.get("brave"))
        .cloned()
        .unwrap_or_else(|| FALLBACK_ENGINE.to_string());

    let encoded = form_urlencoded::byte_serialize(input.as_bytes()).collect::<String>();

    engine.replace("%s", &encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_https_url() {
        let config = load_config();

        assert_eq!(
            resolve_input("https://example.com", &config),
            "https://example.com"
        );
    }

    #[test]
    fn resolves_uppercase_scheme() {
        let config = load_config();

        assert_eq!(
            resolve_input("HTTPS://example.com", &config),
            "https://example.com"
        );
    }

    #[test]
    fn uppercase_scheme_keeps_path_case() {
        let config = load_config();

        assert_eq!(
            resolve_input("HTTP://Example.com/CaseSensitive", &config),
            "http://Example.com/CaseSensitive"
        );
    }

    #[test]
    fn normalizes_uppercase_internal_pages() {
        let config = load_config();

        assert_eq!(resolve_input("AXYS://NEWTAB", &config), "axys://newtab");
        assert_eq!(resolve_input("About:Blank", &config), "about:blank");
    }

    #[test]
    fn resolves_domain() {
        let config = load_config();

        assert_eq!(resolve_input("example.com", &config), "https://example.com");
    }

    #[test]
    fn resolves_search() {
        let config = load_config();

        let result = resolve_input("example browser", &config);

        assert!(result.starts_with("https://search.brave.com/search?q="));

        assert!(result.contains("example"));

        assert!(result.contains("browser"));
    }

    #[test]
    fn resolves_internal_page() {
        let config = load_config();

        assert_eq!(resolve_input("axys://settings", &config), "axys://settings");
    }

    #[test]
    fn resolves_youtube_to_canonical_url() {
        let config = load_config();

        assert_eq!(
            resolve_input("youtube.com", &config),
            "https://www.youtube.com"
        );
    }

    #[test]
    fn keeps_file_urls_untouched() {
        let config = load_config();

        assert_eq!(
            resolve_input("file:///home/user/page.html", &config),
            "file:///home/user/page.html"
        );
    }

    #[test]
    fn resolves_localhost_over_http() {
        let config = load_config();

        assert_eq!(
            resolve_input("localhost:3000", &config),
            "http://localhost:3000"
        );
    }

    #[test]
    fn resolves_ipv4_over_http() {
        let config = load_config();

        assert_eq!(resolve_input("192.168.1.1", &config), "http://192.168.1.1");
        assert_eq!(
            resolve_input("127.0.0.1:8080", &config),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn resolves_ipv6_literal_over_http() {
        let config = load_config();

        assert_eq!(resolve_input("[::1]:8080", &config), "http://[::1]:8080");
    }

    #[test]
    fn resolves_about_blank() {
        let config = load_config();

        assert_eq!(resolve_input("about:blank", &config), "about:blank");
    }

    #[test]
    fn falls_back_when_default_engine_missing() {
        let config = SearchConfig {
            default_engine: "nonexistent".to_string(),
            engines: Default::default(),
        };

        let result = resolve_input("hello world", &config);

        assert!(result.starts_with("https://search.brave.com/search?q="));
    }

    #[test]
    fn default_config_always_has_brave() {
        let config = super::default_config();

        assert!(config.engines.contains_key("brave"));
    }

    #[test]
    fn default_config_has_all_known_engines() {
        let config = super::default_config();

        for (id, _) in super::KNOWN_ENGINES {
            assert!(config.engines.contains_key(*id), "missing engine {id}");
        }
    }

    #[test]
    fn resolves_www_youtube_to_canonical_url() {
        let config = load_config();

        assert_eq!(
            resolve_input("www.youtube.com", &config),
            "https://www.youtube.com"
        );
    }
}
