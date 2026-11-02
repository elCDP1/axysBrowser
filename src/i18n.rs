pub fn normalize_locale(locale: &str) -> &'static str {
    let locale = locale.to_ascii_lowercase();

    if locale.starts_with("es") { "es" } else { "en" }
}

pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(normalize_locale(locale));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

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

    /// Raw `section.key` leaves of a locale file.
    ///
    /// The test must not go through `rust_i18n::t!`: a key missing from
    /// Spanish silently falls back to English there, so the old check passed
    /// even when `es.yml` had lost entries. Reading the YAML itself keeps the
    /// two files honest. `str::lines` already drops a trailing `\r`, so a
    /// CRLF checkout parses the same as an LF one.
    fn translation_keys(contents: &str) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        let mut stack: Vec<&str> = Vec::new();

        for line in contents.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let Some((name, value)) = trimmed.split_once(':') else {
                continue;
            };

            let name = name.trim();

            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                continue;
            }

            let indent = line.len() - line.trim_start().len();
            let depth = indent / 2;

            stack.truncate(depth);

            if value.trim().is_empty() {
                stack.push(name);
            } else {
                let mut path = stack.clone();

                path.push(name);

                keys.insert(path.join("."));
            }
        }

        keys
    }

    fn locale_keys(locale: &str) -> BTreeSet<String> {
        let path = format!("locales/{locale}.yml");
        let contents = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{path}"));

        let keys = translation_keys(&contents);

        assert!(
            keys.len() > 50,
            "{path} looks truncated: only {} keys",
            keys.len()
        );

        keys
    }

    /// The two locale files must define exactly the same keys. A missing
    /// Spanish entry is not caught by `rust_i18n` (it falls back to English),
    /// so it has to fail here.
    #[test]
    fn both_languages_define_the_same_keys() {
        let english = locale_keys("en");
        let spanish = locale_keys("es");

        let missing_in_spanish: Vec<_> = english.difference(&spanish).collect();
        let missing_in_english: Vec<_> = spanish.difference(&english).collect();

        assert!(
            missing_in_spanish.is_empty(),
            "missing from locales/es.yml: {missing_in_spanish:?}"
        );
        assert!(
            missing_in_english.is_empty(),
            "missing from locales/en.yml: {missing_in_english:?}"
        );
    }

    /// Every `"section.key"` string literal in the code that names a
    /// translation must exist in both languages. Without this, a key that
    /// is missing (or a locale file that got truncated) shows up in the UI
    /// as the raw key, and nothing fails.
    #[test]
    fn every_translation_key_in_the_code_exists_in_both_languages() {
        let english = locale_keys("en");
        let spanish = locale_keys("es");

        // Top-level sections of the English file: `about:`, `menu:`, …
        let sections: Vec<&str> = english
            .iter()
            .filter_map(|key| key.split('.').next())
            .collect();

        let mut keys = BTreeSet::new();

        let mut pending = vec![std::path::PathBuf::from("src")];

        while let Some(path) = pending.pop() {
            for entry in std::fs::read_dir(&path).expect("read src").flatten() {
                let path = entry.path();

                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let code = std::fs::read_to_string(&path).expect("read source");

                    // Only the non-test code: tests may mention made-up keys.
                    // Normalize first so a CRLF checkout does not hide the
                    // tests module from the split.
                    let code = code.replace("\r\n", "\n");
                    let code = code
                        .split("#[cfg(test)]\nmod tests")
                        .next()
                        .unwrap_or_default();

                    for literal in code.split('"').skip(1).step_by(2) {
                        // File names ("bookmarks.toml") look like keys too.
                        let is_file = ["toml", "json", "png", "svg", "css", "html", "ftl"]
                            .iter()
                            .any(|ext| literal.ends_with(&format!(".{ext}")));

                        let is_key = !is_file
                            && literal.contains('.')
                            && literal.chars().all(|c| {
                                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'
                            })
                            && sections.contains(&literal.split('.').next().unwrap_or_default());

                        if is_key {
                            keys.insert(literal.to_string());
                        }
                    }
                }
            }
        }

        assert!(
            keys.len() > 50,
            "found only {} keys: scan is broken",
            keys.len()
        );

        for key in keys {
            assert!(
                english.contains(&key),
                "{key} is used in the code but missing from locales/en.yml"
            );
            assert!(
                spanish.contains(&key),
                "{key} is used in the code but missing from locales/es.yml"
            );
        }
    }
}
