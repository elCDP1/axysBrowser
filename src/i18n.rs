pub fn normalize_locale(locale: &str) -> &'static str {
    let locale = locale.to_ascii_lowercase();

    if locale.starts_with("es") { "es" } else { "en" }
}

pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(normalize_locale(locale));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_spanish_locales() {
        for locale in ["es", "ES", "es-ES", "es_MX", "es-419"] {
            assert_eq!(normalize_locale(locale), "es", "{locale}");
        }
    }

    #[test]
    fn falls_back_to_english_for_everything_else() {
        for locale in ["en", "EN", "en-US", "fr", "de-DE", "", "pt-BR"] {
            assert_eq!(normalize_locale(locale), "en", "{locale}");
        }
    }
}
