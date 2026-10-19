# Contributing

Thanks for your interest in axysBrowser.

## Getting set up

Requirements: Rust **1.92+**, GTK 4.10+ and WebKitGTK 6.0 (2.52+) development
files. See the [README](README.md#build) for the exact packages.

```sh
git clone https://github.com/elCDP1/axysBrowser
cd axysBrowser
cargo run --release
```

## Before submitting a pull request

Run the full check suite and make sure it is green:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
xvfb-run -a bash .github/gui-smoke.sh
xvfb-run -a bash .github/mem-smoke.sh
```

CI enforces formatting, tests and clippy (`-D warnings`) on every push.

## Guidelines

- Keep changes focused; one topic per pull request.
- Add or update tests for behavior you change (see the existing
  `#[cfg(test)]` modules for the style used in this project).
- Match the existing code style and keep comments in English.
- Update `locales/en.yml` and `locales/es.yml` together when adding UI text.
- Never commit secrets, credentials or local build artifacts.

## License

By contributing you agree that your contributions are licensed under the
[AXYS Attribution License 1.0](LICENSE). Derivative works must keep visible
attribution to the original project and author.
