# locon

[![crates.io](https://img.shields.io/crates/v/locon.svg)](https://crates.io/crates/locon)
[![docs.rs](https://img.shields.io/docsrs/locon)](https://docs.rs/locon)
[![license](https://img.shields.io/crates/l/locon.svg)](#license)
[![MSRV](https://img.shields.io/crates/msrv/locon.svg)](https://crates.io/crates/locon)

[Locon](https://locon.dev) localization for Rust: flat JSON locale files, lookup **by key or by
phrase**, CLDR plurals, and an asset audit for CI. The files and the lookup rules are the same as in
Locon for JavaScript, so a web or React Native app and Rust code can share one set of translations.

```toml
[dependencies]
locon = "0.1"
```

## Locale files

One flat JSON object per locale, string → string:

```json
// locales/de.json (the project locale: the language the app is written in)
{
  "greet": "Guten Morgen",
  "hello": "Hallo {name}",
  "cards_one": "{count} Karte",
  "cards_other": "{count} Karten"
}
```

```json
// locales/en.json
{
  "greet": "Good morning",
  "hello": "Hello {name}",
  "cards_one": "{count} card",
  "cards_other": "{count} cards"
}
```

- `{token}` placeholders.
- Plural forms are keys with the CLDR category as a suffix: `_zero`, `_one`, `_two`, `_few`,
  `_many`, `_other` (`_other` is required).
- Duplicate keys and non-string values are rejected when loading.

## Usage

```rust
use locon::Locon;

// From a directory at runtime…
let locon = Locon::from_dir("locales", "de")?;
// …or compiled into the binary.
let locon = Locon::from_sources("de", &[
    ("de", include_str!("../locales/de.json")),
    ("en", include_str!("../locales/en.json")),
])?;

locon.set_locale(Some("en"));

locon.l("greet", &[], None, None);                         // by key:    "Good morning"
locon.l("Guten Morgen", &[], None, None);                  // by phrase: "Good morning"
locon.l("Hallo {name}", &[("name", "Ann".into())], None, None); // "Hello Ann"
locon.l("{count} Karten", &[], Some(1.0), None);           // "1 card"
```

Lookup follows Locon:

1. An input that is a key is used as is; otherwise it is looked up by its value in the project locale
   (the first key with that value wins).
2. With a `count`, the plural form of the target locale is chosen with ICU4X plural rules (Russian
   1 / 3 / 5 / 21 all differ); `{count}` is filled in.
3. Fallback per key: target locale → project locale. Empty values count as untranslated. If nothing
   matches, the input itself is returned (interpolated).

### App-wide

```rust
use locon::t;

locon::install(Locon::from_dir("locales", "de")?);
locon::set_locale(Some("en"));

t!("Guten Morgen");                         // "Good morning"
t!("{count} Karten", count = 3);            // "3 cards"  (count first)
t!("Hallo {name}", name = user.name);       // any Display value
```

Without an installed `Locon`, `t!` returns the phrase interpolated, which keeps unit tests readable.

### Device languages

```rust
// The user's preferred languages, most preferred first.
locon.use_device_locales(&["de-AT".into(), "en-US".into()]);   // → "de"
locon::match_locale(&["zh-TW".into()], &["zh-Hans".into(), "zh-Hant".into()]); // → zh-Hant
locon::is_rtl("ar-EG");                                          // true
```

## Asset audit

`locon::check` audits the files against each other and against the phrases the code uses (collected
by your build tool or a source scan):

```rust
let assets = locon::load_dir("locales")?;
let used = vec![("Guten Morgen".to_string(), "src/main.rs".to_string())];
for issue in locon::check(&assets, "de", &used) {
    eprintln!("{}: {}", issue.file, issue.message);
}
```

It reports:

- phrases or keys used by the code that resolve to nothing;
- keys missing from a locale, or unknown to the project locale;
- placeholders added or dropped by a translation (`{count}` may be spelled out in a plural form);
- plural families without `_other`;
- duplicate values whose second key is unreachable by phrase;
- characters of a script the locale does not use (a stray copy-paste).

## License

MIT or Apache-2.0, at your option.
