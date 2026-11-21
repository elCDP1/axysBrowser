//! Pieces both backends share, in engine-neutral terms.
//!
//! The external-scheme decision, the "ask before handing a link to another
//! application" rule, the load-failure page and the encoding helpers are the
//! same for WebKitGTK and for Servo. They live here so a change to that policy
//! cannot land in one backend only: the security-relevant half of the seam is
//! one implementation, tested once.

// Only the document encoders below need it, and they are gated the same way.
#[cfg(any(target_os = "windows", test))]
use std::fmt::Write as _;

/// Schemes the engine never loads itself: handed to the desktop instead
/// (mail clients, dialers, …). Everything else belongs to the engine:
/// http(s), files, internal pages and script/data URLs.
pub(crate) fn is_external_scheme(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();

    let Some(colon) = lower.find(':') else {
        return false;
    };

    // `://foo` or `:foo`: no scheme at all — not something to hand to the
    // desktop.
    if colon == 0 {
        return false;
    }

    if lower[..colon].contains('/') {
        return false;
    }

    !matches!(
        &lower[..colon],
        "http" | "https" | "file" | "axys" | "about" | "blank" | "data" | "blob" | "javascript"
    )
}

/// Links to these schemes open their application without asking: mail links
/// and phone numbers are everywhere and low-risk. Every other external scheme
/// (`ssh:`, `smb:`, `vnc:`, custom handlers …) asks first.
pub(crate) fn external_needs_confirmation(uri: &str) -> bool {
    let scheme = uri
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    !matches!(scheme.as_str(), "mailto" | "tel" | "sms")
}

/// Trims a long URI for a dialog, on a character boundary.
pub(crate) fn shorten_for_dialog(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut shortened: String = text.chars().take(max_chars).collect();

    shortened.push('…');

    shortened
}

/// The page shown when a load fails.
///
/// A page cannot be left blank on a failure: the user needs to know whether
/// the site is down, the certificate is wrong or the network is gone.
pub(crate) fn error_page(title: &str, detail: &str, uri: &str) -> String {
    let esc = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };

    let hint = rust_i18n::t!("error.hint").to_string();

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta name="color-scheme" content="light dark">
<style>
:root{{color-scheme:light dark}}
body{{font-family:system-ui,sans-serif;background:Canvas;color:CanvasText;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}}
.card{{max-width:560px;padding:32px;background:rgba(127,127,127,0.12);border-radius:12px}}
h1{{font-size:20px}}
code{{font-weight:600;word-break:break-all}}
p{{line-height:1.5}}
</style></head><body><div class="card">
<h1>{}</h1><p>{}</p><p><code>{}</code></p><p>{}</p>
</div></body></html>"#,
        esc(title),
        esc(detail),
        esc(uri),
        esc(&hint)
    )
}

// The document encoders below are used by the Servo backend (a generated
// document has to travel as a `data:` URL because Servo has no `load_html`
// with a base) and by the tests in this module. They are gated so a Linux build
// does not carry code its engine never calls -- WebKit loads the document
// itself -- while `cargo test` still checks them on every platform.

/// A `data:` URL for a generated document.
///
/// Servo 0.7 has no `load_html` with a base URL, so the base travels inside
/// the document as a `<base href>` element: relative links and resources then
/// resolve where the caller meant them to.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn data_url(html: &str, base: Option<&str>) -> String {
    let document = match base {
        Some(base) => inject_base(html, base),
        None => html.to_string(),
    };

    format!("data:text/html;charset=utf-8,{}", percent_encode(&document))
}

/// Inserts a `<base href>` element after the document's `<head>` opening tag,
/// or at the front when there is no head to attach to.
#[cfg(any(target_os = "windows", test))]
fn inject_base(html: &str, base: &str) -> String {
    let tag = format!(r#"<base href="{}">"#, escape_attribute(base));

    if let Some(head) = html.to_ascii_lowercase().find("<head")
        && let Some(close) = html[head..].find('>')
    {
        let at = head + close + 1;
        let mut document = String::with_capacity(html.len() + tag.len());

        document.push_str(&html[..at]);
        document.push_str(&tag);
        document.push_str(&html[at..]);

        return document;
    }

    format!("{tag}{html}")
}

#[cfg(any(target_os = "windows", test))]
fn escape_attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Percent-encodes everything a `data:` URL body cannot carry literally.
///
/// Only the unreserved set stays as-is; the rest becomes `%XX`. That is
/// conservative on purpose: `#`, `%`, `?` and `&` all change the meaning of
/// the URL, and a generated document is not worth reasoning about which of
/// them happen to be safe.
#[cfg(any(target_os = "windows", test))]
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());

    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }

    encoded
}

/// A double-quoted JavaScript string literal for `text`.
///
/// The result is embedded in a script that replaces a page's document, so it
/// must not be able to end the literal or the statement: quotes, backslashes,
/// control characters and the HTML-significant `<`, `>` and `&` are escaped,
/// together with the two Unicode line separators a JS literal cannot contain.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn js_string_literal(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);

    literal.push('"');

    for character in text.chars() {
        match character {
            '"' => {
                literal.push('\\');
                literal.push('"');
            }
            '\\' => {
                literal.push('\\');
                literal.push('\\');
            }
            '\n' => {
                literal.push('\\');
                literal.push('n');
            }
            '\r' => {
                literal.push('\\');
                literal.push('r');
            }
            '\t' => {
                literal.push('\\');
                literal.push('t');
            }
            '<' => literal.push_str("\\u003C"),
            '>' => literal.push_str("\\u003E"),
            '&' => literal.push_str("\\u0026"),
            '\u{2028}' => literal.push_str("\\u2028"),
            '\u{2029}' => literal.push_str("\\u2029"),
            control if (control as u32) < 0x20 => {
                let _ = write!(literal, "\\u{:04X}", control as u32);
            }
            other => literal.push(other),
        }
    }

    literal.push('"');

    literal
}

#[cfg(test)]
mod tests {
    use super::{
        data_url, error_page, external_needs_confirmation, is_external_scheme, js_string_literal,
        shorten_for_dialog,
    };

    #[test]
    fn only_web_schemes_belong_to_the_engine() {
        assert!(!is_external_scheme("https://example.com/x"));
        assert!(!is_external_scheme("HTTPS://example.com/x"));
        assert!(!is_external_scheme("axys://about"));
        assert!(!is_external_scheme("data:text/html,x"));
        assert!(!is_external_scheme("javascript:void(0)"));
        assert!(!is_external_scheme("not a url at all"));
        assert!(!is_external_scheme("://no-scheme"));

        assert!(is_external_scheme("mailto:a@example.com"));
        assert!(is_external_scheme("ssh://host/"));
        assert!(is_external_scheme("steam://run/1"));
    }

    #[test]
    fn only_mail_and_phone_links_skip_the_confirmation() {
        assert!(!external_needs_confirmation("mailto:a@example.com"));
        assert!(!external_needs_confirmation("MAILTO:a@example.com"));
        assert!(!external_needs_confirmation("tel:+34123456789"));
        assert!(!external_needs_confirmation("sms:+34123456789"));
        assert!(external_needs_confirmation("ssh://host/"));
        assert!(external_needs_confirmation("smb://host/share"));
        assert!(external_needs_confirmation("vnc://host"));
        assert!(external_needs_confirmation("steam://run/1"));
        assert!(external_needs_confirmation("mailto.evil://x"));
    }

    #[test]
    fn dialog_text_is_shortened_on_a_char_boundary() {
        assert_eq!(shorten_for_dialog("short", 10), "short");
        assert_eq!(shorten_for_dialog("abcdef", 3), "abc…");

        let accents = "ñ".repeat(50);

        assert_eq!(shorten_for_dialog(&accents, 5).chars().count(), 6);
    }

    #[test]
    fn the_error_page_escapes_what_it_is_given() {
        let page = error_page("<title>", "a & b", "https://example.com/<x>");

        assert!(page.contains("&lt;title&gt;"));
        assert!(page.contains("a &amp; b"));
        assert!(page.contains("https://example.com/&lt;x&gt;"));
        assert!(!page.contains("<title>"), "the title must not be markup");
    }

    #[test]
    fn a_generated_document_becomes_a_parseable_data_url() {
        let url = data_url("<h1>hola</h1>", None);

        assert!(url.starts_with("data:text/html;charset=utf-8,"));

        let parsed = url::Url::parse(&url).expect("the data URL must parse");

        assert_eq!(parsed.scheme(), "data");
        assert!(!url.contains("<h1>"), "markup must be percent-encoded");
        assert!(url.contains("%3Ch1%3E"));
    }

    #[test]
    fn a_base_url_travels_inside_the_document() {
        let with_head = data_url(
            "<!doctype html><html><head><meta charset=\"utf-8\"></head><body>x</body></html>",
            Some("https://example.com/a/"),
        );
        let decoded = percent_decode(&with_head);

        assert!(
            decoded.contains(r#"<head><base href="https://example.com/a/">"#),
            "the base must be the first thing in the head: {decoded}"
        );

        // A fragment with no head still gets one, at the front.
        let headless = percent_decode(&data_url("<p>x</p>", Some("https://example.com/")));

        assert!(headless.starts_with(r#"<base href="https://example.com/">"#));

        // A base that could break out of the attribute is escaped.
        let quoted = percent_decode(&data_url(
            "<head></head>",
            Some("https://e.test/\"onload=x"),
        ));

        assert!(quoted.contains("&quot;onload=x"));
    }

    #[test]
    fn a_script_literal_cannot_end_itself() {
        let quote = '"';
        let backslash = '\\';
        let input = format!("a{quote}b{backslash}c</script>&d\u{2028}e");

        let literal = js_string_literal(&input);

        assert!(literal.starts_with(quote) && literal.ends_with(quote));

        // A quote and a backslash are always escaped, so the literal cannot be
        // closed from inside the data.
        let escaped_quote: String = [backslash, quote].iter().collect();

        assert!(
            literal.contains(&escaped_quote),
            "a quote must be escaped: {literal}"
        );
        assert!(literal.contains("u003C"));
        assert!(literal.contains("u0026"));
        assert!(literal.contains("u2028"));
        assert!(!literal.contains('\u{2028}'), "a raw separator breaks JS");
    }

    /// Minimal decoder for the tests above: `%XX` back to bytes.
    fn percent_decode(url: &str) -> String {
        let body = url
            .split_once(',')
            .map(|(_head, body)| body)
            .unwrap_or_default();
        let bytes = body.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;

        while index < bytes.len() {
            if bytes[index] == b'%' && index + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or_default();

                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    decoded.push(value);
                    index += 3;

                    continue;
                }
            }

            decoded.push(bytes[index]);
            index += 1;
        }

        String::from_utf8_lossy(&decoded).into_owned()
    }
}
