use serde::Deserialize;
use std::{collections::HashMap, fs};
use url::form_urlencoded;

#[derive(Clone, Debug, Deserialize)]
pub struct SearchConfig {
    pub default_engine: String,
    pub engines: HashMap<String, String>,
}

const DEFAULT_CONFIG: &str = include_str!("../../config/search.toml");

const FALLBACK_ENGINE: &str = "https://search.brave.com/search?q=%s";

fn default_config() -> SearchConfig {
    toml::from_str(DEFAULT_CONFIG).unwrap_or_else(|_| SearchConfig {
        default_engine: "brave".to_string(),
        engines: [("brave".to_string(), FALLBACK_ENGINE.to_string())].into(),
    })
}

pub fn load_config() -> SearchConfig {
    // Never panic on a user-edited config: fall back to the embedded default,
    // and if even that is broken, to a hardcoded Brave entry.
    let text =
        fs::read_to_string("config/search.toml").unwrap_or_else(|_| DEFAULT_CONFIG.to_string());

    toml::from_str(&text).unwrap_or_else(|error| {
        eprintln!("Invalid search configuration, using defaults: {error}");

        default_config()
    })
}

pub fn resolve_input(input: &str, config: &SearchConfig) -> String {
    let input = input.trim();

    if input.is_empty() {
        return "axys://newtab".to_string();
    }

    if input.starts_with("axys://") {
        return input.to_string();
    }

    if input == "about:blank" {
        return input.to_string();
    }

    if input.starts_with("http://") || input.starts_with("https://") {
        return input.to_string();
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
    fn resolves_www_youtube_to_canonical_url() {
        let config = load_config();

        assert_eq!(
            resolve_input("www.youtube.com", &config),
            "https://www.youtube.com"
        );
    }
}
