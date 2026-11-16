# locon (Rust)

[Locon](https://locon.dev) localization for [RealNative](https://github.com/vladbars/realnative) apps,
in Rust. Same locale files and semantics as Locon for JavaScript, so React Native apps that use Locon
keep their `locales/*.json` unchanged.

- Flat `locales/<locale>.json` (string → string), duplicate keys rejected.
- Lookup by key **or by phrase** (the project-locale value), `{token}` placeholders, CLDR plural forms
  (`_zero/_one/_two/_few/_many/_other`) with ICU4X plural rules, per-key fallback to the project locale.
- Device language matching (`de-AT` → `de`, `zh-TW` → `zh-Hant`), right-to-left detection.
- The Locon asset audit (missing/unknown keys, placeholders, plural `_other`, unreachable duplicates,
  stray scripts, unresolved phrases) for `realnative check`.

## In a RealNative app

`app-logic/Cargo.toml`: `locon` and `include_dir = "0.7.4"`, then once in `app-logic`:

```rust
locon::setup!(source = "en");
use locon::t;

let label = t!("{count} cards", count = n);     // plural form of the current locale
let hello = t!("Hello {name}", name = user.name);
locon::set_locale(Some("de"));        // switches the whole app at once
```

UI code uses `tr!("Good morning")` (a phrase or a key); `realnative generate` resolves it to its key
and the generated SwiftUI / Compose code reads the string from a table app logic hands over in one
call — at start and after each language change.

## Differences from Locon JS

- Phrases in UI code are resolved at generate time; an unresolved one is a `realnative check` error.
- The fallback chain is current locale → project locale (Locon JS also has a separate default locale).
- `count` is written first in `t!`.

License: MIT OR Apache-2.0.
