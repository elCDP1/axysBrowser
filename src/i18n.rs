pub fn normalize_locale(locale: &str) -> &'static str {
    let locale = locale.to_ascii_lowercase();

    if locale.starts_with("es") { "es" } else { "en" }
}

pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(normalize_locale(locale));
}
