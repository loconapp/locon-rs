//! [Locon](https://locon.dev) localization for Rust.
//!
//! - One flat JSON object per locale (`locales/de.json`: `{"greet": "Guten Morgen"}`): the same files
//!   as Locon for JavaScript, so web, mobile and Rust code can share them.
//! - Lookup **by key or by phrase**: an input that is not a key is looked up by its value in the
//!   project (source) locale, so code can read `l("Guten Morgen")`.
//! - `{token}` placeholders; CLDR plural forms as `_zero/_one/_two/_few/_many/_other` key suffixes,
//!   chosen with ICU4X plural rules; `{count}` filled automatically.
//! - Per-key fallback to the project locale; device language matching (`de-AT` → `de`,
//!   `zh-TW` → `zh-Hant`); right-to-left detection.
//! - [`check`]: the Locon asset audit (missing and unknown keys, placeholders, plural `_other`,
//!   unreachable duplicates, stray scripts, unresolved phrases), for CI.
//!
//! ```
//! let locon = locon::Locon::from_sources("de", &[
//!     ("de", r#"{"cards_one": "{count} Karte", "cards_other": "{count} Karten"}"#),
//!     ("en", r#"{"cards_one": "{count} card", "cards_other": "{count} cards"}"#),
//! ]).unwrap();
//! locon.set_locale(Some("en"));
//! assert_eq!(locon.l("{count} Karten", &[], Some(3.0), None), "3 cards");
//! ```
//!
//! For app-wide use, [`install`] one `Locon` and format with [`t!`].

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

mod check;
mod locale;
pub use check::{Issue, check};
pub use locale::{is_rtl, match_locale};

/// locale → key → value.
pub type Assets = BTreeMap<String, BTreeMap<String, String>>;

const PLURAL_SUFFIXES: [&str; 6] = ["_zero", "_one", "_two", "_few", "_many", "_other"];

/// `key_one` → `key`; other keys unchanged.
pub fn plural_base(key: &str) -> &str {
    PLURAL_SUFFIXES
        .iter()
        .find_map(|s| key.strip_suffix(s))
        .unwrap_or(key)
}

pub fn is_plural_key(key: &str) -> bool {
    plural_base(key) != key
}

/// Parses one locale file: a flat JSON object of strings, without duplicate keys.
pub fn parse_locale(json: &str) -> Result<BTreeMap<String, String>, String> {
    struct Flat(BTreeMap<String, String>);
    impl<'de> serde::Deserialize<'de> for Flat {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Flat;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a flat JSON object of strings")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut m: A,
                ) -> Result<Flat, A::Error> {
                    let mut out = BTreeMap::new();
                    while let Some((k, v)) = m.next_entry::<String, serde_json::Value>()? {
                        let serde_json::Value::String(v) = v else {
                            return Err(serde::de::Error::custom(format!(
                                "\"{k}\" is not a string"
                            )));
                        };
                        if out.insert(k.clone(), v).is_some() {
                            return Err(serde::de::Error::custom(format!("duplicate key \"{k}\"")));
                        }
                    }
                    Ok(Flat(out))
                }
            }
            d.deserialize_map(V)
        }
    }
    serde_json::from_str::<Flat>(json)
        .map(|f| f.0)
        .map_err(|e| e.to_string())
}

/// A locale file that cannot be read or is not a flat object of strings.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadError {
    /// The file (or locale) it is about.
    pub file: String,
    pub message: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

impl std::error::Error for LoadError {}

/// Reads every `<locale>.json` in a directory.
pub fn load_dir(dir: impl AsRef<std::path::Path>) -> Result<Assets, LoadError> {
    let dir = dir.as_ref();
    let mut out = Assets::new();
    let rd = std::fs::read_dir(dir).map_err(|e| LoadError {
        file: dir.display().to_string(),
        message: e.to_string(),
    })?;
    let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
    {
        let locale = p.file_stem().unwrap().to_string_lossy().to_string();
        let file = p.display().to_string();
        let text = std::fs::read_to_string(&p).map_err(|e| LoadError {
            file: file.clone(),
            message: e.to_string(),
        })?;
        out.insert(
            locale,
            parse_locale(&text).map_err(|message| LoadError { file, message })?,
        );
    }
    Ok(out)
}

/// Locale files given as `(locale, json)` pairs, e.g. compiled in with `include_str!`.
pub fn load_sources(sources: &[(&str, &str)]) -> Result<Assets, LoadError> {
    let mut out = Assets::new();
    for (locale, json) in sources {
        let map = parse_locale(json).map_err(|message| LoadError {
            file: format!("{locale}.json"),
            message,
        })?;
        out.insert(locale.to_string(), map);
    }
    Ok(out)
}

/// CLDR plural category of `count` in `locale` (`"one"`, `"few"`, `"other"`, ...).
pub fn plural_category(count: f64, locale: &str) -> &'static str {
    use icu_plurals::{PluralCategory, PluralOperands, PluralRules};
    let Ok(loc) = locale.replace('_', "-").parse::<icu_locale_core::Locale>() else {
        return "other";
    };
    let Ok(rules) = PluralRules::try_new_cardinal(loc.into()) else {
        return "other";
    };
    let ops: PluralOperands = if count.fract() == 0.0 && count.abs() < 1e15 {
        PluralOperands::from(count.abs() as u64)
    } else {
        // "1.5" keeps its visible fraction digits, which some rules use.
        match fixed_decimal::Decimal::try_from_str(&format!("{}", count.abs())) {
            Ok(d) => PluralOperands::from(&d),
            Err(_) => return "other",
        }
    };
    match rules.category_for(ops) {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

/// Replaces `{token}` placeholders; unknown tokens stay as they are.
pub fn interpolate(value: &str, params: &[(&str, String)]) -> String {
    if params.is_empty() {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close)
                if !after[..close].is_empty()
                    && after[..close]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_') =>
            {
                let token = &after[..close];
                match params.iter().find(|(k, _)| *k == token) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(token);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The localization of one app: its locale files, project locale and current locale.
pub struct Locon {
    assets: Assets,
    project: String,
    /// value → first key with that value, per locale (phrase lookup in O(1)).
    reverse: HashMap<String, HashMap<String, String>>,
    current: RwLock<String>,
    revision: AtomicU64,
}

impl Locon {
    /// Loads `<dir>/<locale>.json`; `project` is the source language (Locon `projectLocale`).
    ///
    /// ```no_run
    /// let locon = locon::Locon::from_dir("locales", "en")?;
    /// # Ok::<(), locon::LoadError>(())
    /// ```
    pub fn from_dir(dir: impl AsRef<std::path::Path>, project: &str) -> Result<Self, LoadError> {
        Ok(Self::new(load_dir(dir)?, project))
    }

    /// Locale files compiled into the binary:
    ///
    /// ```ignore
    /// let locon = locon::Locon::from_sources("en", &[
    ///     ("en", include_str!("../locales/en.json")),
    ///     ("de", include_str!("../locales/de.json")),
    /// ])?;
    /// ```
    pub fn from_sources(project: &str, sources: &[(&str, &str)]) -> Result<Self, LoadError> {
        Ok(Self::new(load_sources(sources)?, project))
    }

    /// `project` is the source language (Locon `projectLocale`).
    pub fn new(assets: Assets, project: &str) -> Self {
        let mut reverse = HashMap::new();
        for (loc, map) in &assets {
            let mut idx = HashMap::new();
            for (k, v) in map {
                // First key wins (BTreeMap order), as in Locon: duplicates resolve deterministically.
                idx.entry(v.clone()).or_insert_with(|| k.clone());
            }
            reverse.insert(loc.clone(), idx);
        }
        Self {
            assets,
            project: project.to_string(),
            reverse,
            current: RwLock::new(project.to_string()),
            revision: AtomicU64::new(0),
        }
    }

    pub fn assets(&self) -> &Assets {
        &self.assets
    }

    pub fn project_locale(&self) -> &str {
        &self.project
    }

    pub fn locales(&self) -> Vec<String> {
        self.assets.keys().cloned().collect()
    }

    pub fn locale(&self) -> String {
        self.current.read().unwrap().clone()
    }

    /// Changes the current locale; `None` or an unknown locale selects the project locale.
    /// Bumps [`Locon::revision`] when it changes.
    pub fn set_locale(&self, locale: Option<&str>) {
        let next = locale
            .and_then(|l| match_locale(&[l.to_string()], &self.locales()))
            .unwrap_or_else(|| self.project.clone());
        let mut cur = self.current.write().unwrap();
        if *cur != next {
            *cur = next;
            self.revision.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Picks the first of the device's preferred languages the app has, else the project locale.
    pub fn use_device_locales(&self, preferred: &[String]) {
        let l = match_locale(preferred, &self.locales());
        self.set_locale(l.as_deref());
    }

    /// Changes with every locale change: callers that cache strings reload when it moves.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    fn read(&self, locale: &str, key: &str) -> Option<&str> {
        // Empty values count as untranslated so the fallback continues.
        self.assets
            .get(locale)?
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// The asset key for a key or a project-locale phrase; the input itself when neither matches.
    pub fn key_for(&self, input: &str, target: &str) -> String {
        if self.read(target, input).is_some()
            || self
                .assets
                .get(&self.project)
                .is_some_and(|m| m.contains_key(input))
        {
            return input.to_string();
        }
        self.reverse
            .get(&self.project)
            .and_then(|r| r.get(input))
            .cloned()
            .unwrap_or_else(|| input.to_string())
    }

    /// The key a static string resolves to (an exact key or a project-locale phrase), or `None`
    /// when it resolves to nothing. Build tools use it to turn phrases into keys ahead of time.
    pub fn resolve_static(&self, input: &str) -> Option<String> {
        let src = self.assets.get(&self.project)?;
        if src.contains_key(input) {
            return Some(input.to_string());
        }
        self.reverse.get(&self.project)?.get(input).cloned()
    }

    /// Locon `l()`: lookup by key or phrase, `{token}` params, plural by `count`, per-key fallback.
    pub fn l(
        &self,
        input: &str,
        params: &[(&str, String)],
        count: Option<f64>,
        locale: Option<&str>,
    ) -> String {
        if input.is_empty() {
            return String::new();
        }
        let target = locale.map(str::to_string).unwrap_or_else(|| self.locale());
        let resolved = self.key_for(input, &target);
        let key = if count.is_some() {
            plural_base(&resolved).to_string()
        } else {
            resolved
        };
        let mut params: Vec<(&str, String)> = params.to_vec();
        if let Some(c) = count {
            params.retain(|(k, _)| *k != "count");
            params.push(("count", format_count(c)));
        }
        for cand in [target.as_str(), self.project.as_str()] {
            let keys: Vec<String> = match count {
                None => vec![key.clone()],
                Some(c) => vec![
                    format!("{key}_{}", plural_category(c, cand)),
                    format!("{key}_other"),
                    key.clone(),
                ],
            };
            for k in keys {
                if let Some(v) = self.read(cand, &k) {
                    return interpolate(v, &params);
                }
            }
        }
        interpolate(input, &params)
    }

    /// Every key of the project locale with its value in `locale` (per-key fallback), e.g. to hand
    /// a whole string table to a UI layer.
    pub fn table(&self, locale: &str) -> HashMap<String, String> {
        let Some(src) = self.assets.get(&self.project) else {
            return HashMap::new();
        };
        src.keys()
            .map(|k| {
                let v = self
                    .read(locale, k)
                    .or_else(|| self.read(&self.project, k))
                    .unwrap_or("");
                (k.clone(), v.to_string())
            })
            .collect()
    }
}

fn format_count(c: f64) -> String {
    if c.fract() == 0.0 && c.abs() < 1e15 {
        format!("{}", c as i64)
    } else {
        format!("{c}")
    }
}

static GLOBAL: OnceLock<Locon> = OnceLock::new();

/// Makes `locon` the app-wide localization used by [`t()`] and [`t!`] (first call wins).
pub fn install(locon: Locon) -> &'static Locon {
    let _ = GLOBAL.set(locon);
    GLOBAL.get().unwrap()
}

/// The installed localization, if any.
pub fn global() -> Option<&'static Locon> {
    GLOBAL.get()
}

/// Formats text through the installed localization ([`install`]). Without one the phrase is
/// interpolated as it is, like Locon's last resort (handy in unit tests).
pub fn t(phrase: &str, params: &[(&str, String)], count: Option<f64>) -> String {
    match global() {
        Some(l) => l.l(phrase, params, count, None),
        None => {
            let mut params = params.to_vec();
            if let Some(c) = count {
                params.push(("count", format_count(c)));
            }
            interpolate(phrase, &params)
        }
    }
}

/// Changes the language of the installed localization; `None` selects the project locale.
pub fn set_locale(locale: Option<&str>) {
    if let Some(l) = global() {
        l.set_locale(locale);
    }
}

/// The current locale of the installed localization.
pub fn locale() -> Option<String> {
    global().map(Locon::locale)
}

/// Localized text through the installed localization: `t!("Good morning")`, `t!("{count} cards", count = n)`,
/// `t!("Hello {name}", name = user.name)`. `count` (written first) selects the plural form and fills
/// `{count}`; other values are anything with `Display`.
#[macro_export]
macro_rules! t {
    ($phrase:expr) => {
        $crate::t($phrase, &[], None)
    };
    ($phrase:expr, count = $count:expr $(, $name:ident = $value:expr)* $(,)?) => {{
        let params: Vec<(&str, String)> = vec![$((stringify!($name), $value.to_string())),*];
        $crate::t($phrase, &params, Some($crate::__count(&$count)))
    }};
    ($phrase:expr, $($name:ident = $value:expr),+ $(,)?) => {{
        let params: Vec<(&str, String)> = vec![$((stringify!($name), $value.to_string())),+];
        $crate::t($phrase, &params, None)
    }};
}

#[doc(hidden)]
pub trait Count {
    fn as_count(&self) -> f64;
}
macro_rules! count_impl {
    ($($t:ty),*) => { $(impl Count for $t { fn as_count(&self) -> f64 { *self as f64 } })* };
}
count_impl!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);
#[doc(hidden)]
pub fn __count<C: Count>(c: &C) -> f64 {
    c.as_count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> Assets {
        let mut a = Assets::new();
        a.insert("de".into(), parse_locale(r#"{"greet":"Guten Morgen","cards_one":"{count} Karte","cards_other":"{count} Karten","hello":"Hallo {name}","only_de":"Nur","empty":""}"#).unwrap());
        a.insert("en".into(), parse_locale(r#"{"greet":"Good morning","cards_one":"{count} card","cards_other":"{count} cards","hello":"Hello {name}","empty":"Empty"}"#).unwrap());
        a.insert("ru".into(), parse_locale(r#"{"greet":"Доброе утро","cards_one":"{count} карточка","cards_few":"{count} карточки","cards_many":"{count} карточек","cards_other":"{count} карточки"}"#).unwrap());
        a
    }

    #[test]
    fn phrase_or_key_plurals_params_fallback() {
        let l = Locon::new(assets(), "de");
        l.set_locale(Some("en"));
        assert_eq!(l.l("Guten Morgen", &[], None, None), "Good morning");
        assert_eq!(l.l("greet", &[], None, None), "Good morning");
        assert_eq!(l.l("{count} Karten", &[], Some(1.0), None), "1 card");
        assert_eq!(l.l("cards", &[], Some(3.0), None), "3 cards");
        assert_eq!(
            l.l("Hallo {name}", &[("name", "Ann".into())], None, None),
            "Hello Ann"
        );
        // Missing in en → project locale (per key); unknown phrase → itself.
        assert_eq!(l.l("Nur", &[], None, None), "Nur");
        assert_eq!(
            l.l("Nicht da {x}", &[("x", "1".into())], None, None),
            "Nicht da 1"
        );
        // Empty in the project locale counts as untranslated.
        assert_eq!(l.l("empty", &[], None, Some("de")), "empty");
        l.set_locale(Some("ru"));
        for (n, want) in [
            (1.0, "1 карточка"),
            (3.0, "3 карточки"),
            (5.0, "5 карточек"),
            (21.0, "21 карточка"),
        ] {
            assert_eq!(l.l("{count} Karten", &[], Some(n), None), want);
        }
    }

    #[test]
    fn static_resolution_and_table() {
        let l = Locon::new(assets(), "de");
        assert_eq!(l.resolve_static("Guten Morgen").as_deref(), Some("greet"));
        assert_eq!(l.resolve_static("greet").as_deref(), Some("greet"));
        assert_eq!(l.resolve_static("Good morning"), None);
        let t = l.table("ru");
        assert_eq!(
            (t["greet"].as_str(), t["only_de"].as_str()),
            ("Доброе утро", "Nur")
        );
    }

    #[test]
    fn locale_switch_bumps_revision() {
        let l = Locon::new(assets(), "de");
        assert_eq!(l.revision(), 0);
        l.use_device_locales(&["fr-FR".into(), "en-GB".into()]);
        assert_eq!((l.locale().as_str(), l.revision()), ("en", 1));
        l.set_locale(Some("en"));
        assert_eq!(l.revision(), 1);
        l.set_locale(None);
        assert_eq!(l.locale(), "de");
    }

    #[test]
    fn duplicate_keys_and_non_strings_are_rejected() {
        assert!(
            parse_locale(r#"{"a":"x","a":"y"}"#)
                .unwrap_err()
                .contains("duplicate key")
        );
        assert!(
            parse_locale(r#"{"a":1}"#)
                .unwrap_err()
                .contains("not a string")
        );
    }

    #[test]
    fn t_without_installation_interpolates() {
        assert_eq!(t!("{count} cards", count = 2), "2 cards");
        assert_eq!(t!("Hi {name}", name = "Bo"), "Hi Bo");
    }
}
