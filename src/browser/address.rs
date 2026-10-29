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

/// Whether a dotted input should be treated as a hostname instead of a search.
///
/// Nearly everything with a dot and no spaces is a URL, but pure numbers
/// (`3.14`, `version.2`) and near-miss addresses (`e.g`) are almost always
/// searches; guessing `https://3.14` only produced a load error. The last
/// label must look like a domain: at least two characters and not all
/// digits. A single trailing dot (fully-qualified name) is ignored, and IDN
/// labels are accepted because the check is "not numeric", not "ASCII".
fn looks_like_host(input: &str) -> bool {
    if input.contains(' ') {
        return false;
    }

    let host = input.split(['/', '?', '#']).next().unwrap_or(input);

    // Drop a numeric port before looking at the last label.
    let host = match host.rsplit_once(':') {
        Some((head, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => head,
        _ => host,
    };

    let host = host.strip_suffix('.').unwrap_or(host);

    let Some(last_label) = host.rsplit('.').next() else {
        return false;
    };

    last_label.chars().count() >= 2 && !last_label.chars().all(|c| c.is_ascii_digit())
}

pub fn resolve_input(input: &str, config: &SearchConfig) -> String {
    let input = input.trim();

    if input.is_empty() {
        return "axys://newtab".to_string();
    }

    let lower = input.to_ascii_lowercase();

    // `view-source:` is a display mode, not a navigation target. The source
    // tab labels itself `view-source:<url>`, so resolving that verbatim built
    // a broken `https://view-source:...` URL when the user pressed Enter.
    // Resolve the inner address instead, which loads the page itself.
    const VIEW_SOURCE: &str = "view-source:";

    if lower.starts_with(VIEW_SOURCE) {
        return resolve_input(&input[VIEW_SOURCE.len()..], config);
    }

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

    // A scheme we cannot load (`ftp://`, `ssh://`, …) would otherwise be
    // turned into `https://ftp://…` by the heuristic below, which only ends
    // in a load error. Searching it keeps the user's intent.
    if let Some(colon) = lower.find("://") {
        let scheme = &lower[..colon];

        if !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            return search_url(input, config);
        }
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

    if input.contains('.') && looks_like_host(input) {
        let host = input.trim_end_matches('/');

        if host.eq_ignore_ascii_case("youtube.com") {
            return "https://www.youtube.com".to_string();
        }

        if host.eq_ignore_ascii_case("www.youtube.com") {
            return "https://www.youtube.com".to_string();
        }

        return format!("https://{input}");
    }

    search_url(input, config)
}

/// Sends `input` to the configured engine, form-encoding it as a query
/// (spaces become `+`, which every bundled engine accepts).
fn search_url(input: &str, config: &SearchConfig) -> String {
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
    fn resolves_view_source_to_the_inner_address() {
        let config = load_config();

        assert_eq!(
            resolve_input("view-source:https://example.com/Path", &config),
            "https://example.com/Path"
        );
        assert_eq!(
            resolve_input("VIEW-SOURCE:example.com", &config),
            "https://example.com"
        );
        assert_eq!(
            resolve_input("view-source:", &config),
            "axys://newtab",
            "an empty source address falls back to the new tab"
        );
    }

    #[test]
    fn resolves_www_youtube_to_canonical_url() {
        let config = load_config();

        assert_eq!(
            resolve_input("www.youtube.com", &config),
            "https://www.youtube.com"
        );
    }

    #[test]
    fn search_uses_the_selected_engine() {
        let mut config = load_config();

        config.default_engine = "duckduckgo".to_string();

        let resolved = resolve_input("hola mundo", &config);

        // `byte_serialize` is form encoding: spaces become `+`, which every
        // configured engine accepts in its query string.
        assert_eq!(resolved, "https://duckduckgo.com/?q=hola+mundo");
        assert!(!resolved.contains("%s"), "placeholder must be replaced");
    }

    #[test]
    fn unknown_schemes_are_searched_instead_of_becoming_https_urls() {
        let config = load_config();

        for input in ["ftp://example.com/file", "ssh://example.com"] {
            let resolved = resolve_input(input, &config);

            assert!(
                resolved.starts_with("https://search.brave.com/search?q="),
                "{input} must be searched, got {resolved}"
            );
        }
    }

    #[test]
    fn dotted_numbers_are_searched_instead_of_becoming_hosts() {
        let config = load_config();

        for input in ["3.14", "1.5", "version.2", "10.0.0.999"] {
            let resolved = resolve_input(input, &config);

            assert!(
                resolved.starts_with("https://search.brave.com/search?q="),
                "{input} must be searched, got {resolved}"
            );
        }
    }

    #[test]
    fn near_miss_domains_are_searched() {
        let config = load_config();

        for input in ["e.g", "e.g.", "example.c"] {
            let resolved = resolve_input(input, &config);

            assert!(
                resolved.starts_with("https://search.brave.com/search?q="),
                "{input} must be searched, got {resolved}"
            );
        }
    }

    #[test]
    fn host_with_port_is_still_a_url() {
        let config = load_config();

        assert_eq!(
            resolve_input("example.com:8080/x", &config),
            "https://example.com:8080/x"
        );
    }

    #[test]
    fn fully_qualified_host_keeps_its_trailing_dot() {
        let config = load_config();

        assert_eq!(
            resolve_input("Example.com./Path", &config),
            "https://Example.com./Path"
        );
    }
}
