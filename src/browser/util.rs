//! Small helpers shared by the browser core.
//!
//! Nothing here knows about an engine: these are facts about text and URLs
//! that both the WebKit and the Servo backends need, which is why they do not
//! live in `browser::engine`.

/// URI form that is safe to write to stderr: scheme, host and port only.
/// Credentials, path, query and fragment are dropped, because popup URLs and
/// external links can carry session tokens and end up in the journal.
pub(crate) fn log_safe_uri(uri: &str) -> String {
    let Some(colon) = uri.find(':') else {
        return "(url)".to_string();
    };

    let scheme = &uri[..colon];

    let Some(rest) = uri[colon + 1..].strip_prefix("//") else {
        // `mailto:`, `tel:`, …: the scheme alone is enough for a log line.
        return format!("{scheme}:…");
    };

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());

    // A `user:password@host` authority would leak credentials: keep the host.
    let authority = &rest[..authority_end];

    let host = authority.rsplit('@').next().unwrap_or(authority);

    if host.is_empty() {
        format!("{scheme}://…")
    } else {
        format!("{scheme}://{host}")
    }
}

#[cfg(test)]
mod tests {
    use super::log_safe_uri;

    #[test]
    fn logged_urls_drop_path_query_and_credentials() {
        assert_eq!(
            log_safe_uri("https://example.com/path?token=secret#fragment"),
            "https://example.com"
        );
        assert_eq!(
            log_safe_uri("https://user:password@example.com:8443/x"),
            "https://example.com:8443"
        );
        assert_eq!(log_safe_uri("ssh://user@host/etc/passwd"), "ssh://host");
        assert_eq!(log_safe_uri("https://[::1]:8080/x"), "https://[::1]:8080");
        assert_eq!(log_safe_uri("mailto:someone@example.com"), "mailto:…");
        assert_eq!(log_safe_uri("https://"), "https://…");
        assert_eq!(log_safe_uri("not a url"), "(url)");
    }
}
