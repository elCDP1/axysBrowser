rust_i18n::i18n!("locales", fallback = "en");

mod app;
mod app_state;
mod browser;
mod i18n;
mod internal;
mod theme;
mod ui;

fn main() {
    app::run();
}
