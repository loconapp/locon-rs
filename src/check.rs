//! The Locon asset audit (`check-locon-assets` of the locon-sync skill), for `realnative check`.

use crate::{Assets, is_plural_key, plural_base};
use std::collections::{BTreeMap, BTreeSet};

/// One problem, with the file it is about (`locales/<locale>.json`, or the source file of a phrase).
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub file: String,
    pub message: String,
}

fn placeholders(v: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = v;
    while let Some(o) = rest.find('{') {
        let after = &rest[o + 1..];
        match after.find('}') {
            Some(c)
                if !after[..c].is_empty()
                    && after[..c]
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_') =>
            {
                out.insert(after[..c].to_string());
                rest = &after[c + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// Characters of a script that only some languages use (stray copy-paste from another locale).
/// (name, code point ranges, languages that use it, explicit script subtags that allow it)
type Script = (
    &'static str,
    &'static [(u32, u32)],
    &'static [&'static str],
    &'static [&'static str],
);

const SCRIPTS: &[Script] = &[
    (
        "Han",
        &[(0x4E00, 0x9FFF), (0x3400, 0x4DBF)],
        &["ja", "ko", "zh"],
        &["hani", "hans", "hant", "jpan", "kore"],
    ),
    ("Japanese Kana", &[(0x3040, 0x30FF)], &["ja"], &["jpan"]),
    (
        "Hangul",
        &[(0xAC00, 0xD7AF), (0x1100, 0x11FF)],
        &["ko"],
        &["kore"],
    ),
    (
        "Cyrillic",
        &[(0x0400, 0x04FF)],
        &["ru", "uk", "be", "bg", "sr", "mk", "kk"],
        &["cyrl"],
    ),
    (
        "Arabic",
        &[(0x0600, 0x06FF)],
        &[
            "ar", "azb", "ckb", "fa", "ks", "lrc", "mzn", "ps", "sd", "ug", "ur",
        ],
        &["arab", "aran"],
    ),
    ("Hebrew", &[(0x0590, 0x05FF)], &["he", "yi"], &["hebr"]),
    (
        "Devanagari",
        &[(0x0900, 0x097F)],
        &["hi", "mr", "ne"],
        &["deva"],
    ),
    ("Thai", &[(0x0E00, 0x0E7F)], &["th"], &["thai"]),
    ("Greek", &[(0x0370, 0x03FF)], &["el"], &["grek"]),
];

/// Audits the locale files against each other and against the phrases the code uses
/// (`used`: phrase or key → file). Returns every problem found.
pub fn check(assets: &Assets, source: &str, used: &[(String, String)]) -> Vec<Issue> {
    let mut out = vec![];
    let file = |l: &str| format!("locales/{l}.json");
    let Some(src) = assets.get(source) else {
        return vec![Issue {
            file: file(source),
            message: format!("missing locale file for the source language `{source}`"),
        }];
    };

    // Plural families need `_other`.
    let families: BTreeSet<&str> = src
        .keys()
        .filter(|k| is_plural_key(k))
        .map(|k| plural_base(k))
        .collect();
    for f in &families {
        if !src.contains_key(&format!("{f}_other")) {
            out.push(Issue {
                file: file(source),
                message: format!("plural family \"{f}\" must define {f}_other as its catch-all"),
            });
        }
    }

    // Every phrase / key used by the code resolves.
    let mut value_to_key: BTreeMap<&str, &str> = BTreeMap::new();
    let mut shadowed = vec![];
    for (k, v) in src {
        match value_to_key.get(v.as_str()) {
            Some(first) => shadowed.push((v.clone(), first.to_string(), k.clone())),
            None => {
                value_to_key.insert(v, k);
            }
        }
    }
    let mut used_keys = BTreeSet::new();
    for (phrase, f) in used {
        if src.contains_key(phrase) {
            used_keys.insert(phrase.clone());
        } else if let Some(k) = value_to_key.get(phrase.as_str()) {
            used_keys.insert(k.to_string());
        } else if families.contains(phrase.as_str()) {
            used_keys.extend(src.keys().filter(|k| plural_base(k) == phrase).cloned());
        } else {
            out.push(Issue {
                file: f.clone(),
                message: format!(
                    "\"{phrase}\" is neither a key nor a phrase of locales/{source}.json"
                ),
            });
        }
    }
    // A duplicate value is fine while its second key is used by key; otherwise nothing reaches it.
    for (v, first, second) in shadowed {
        if !used_keys.contains(&second) && !used.iter().any(|(p, _)| p == &second) {
            out.push(Issue { file: file(source), message: format!("\"{v}\" is both {first} and {second}: only {first} is reachable by the phrase, and {second} is not used by key") });
        }
    }

    for (loc, map) in assets {
        if loc != source {
            let missing: Vec<&String> = src
                .keys()
                .filter(|k| {
                    !map.contains_key(*k)
                        && !(is_plural_key(k)
                            && families.contains(plural_base(k))
                            && map.keys().any(|m| plural_base(m) == plural_base(k)))
                })
                .collect();
            if !missing.is_empty() {
                let shown: Vec<&str> = missing.iter().take(5).map(|s| s.as_str()).collect();
                out.push(Issue {
                    file: file(loc),
                    message: format!(
                        "missing {} key(s): {}{}",
                        missing.len(),
                        shown.join(", "),
                        if missing.len() > 5 { " …" } else { "" }
                    ),
                });
            }
            let unknown: Vec<&String> = map
                .keys()
                .filter(|k| {
                    !src.contains_key(*k)
                        && !(is_plural_key(k) && families.contains(plural_base(k)))
                })
                .collect();
            if !unknown.is_empty() {
                let shown: Vec<&str> = unknown.iter().take(5).map(|s| s.as_str()).collect();
                out.push(Issue {
                    file: file(loc),
                    message: format!("unknown key(s): {}", shown.join(", ")),
                });
            }
        }
        for (k, v) in map {
            let sk = if is_plural_key(k) {
                format!("{}_other", plural_base(k))
            } else {
                k.clone()
            };
            let Some(sv) = src.get(&sk) else { continue };
            let (want, got) = (placeholders(sv), placeholders(v));
            let introduced: Vec<_> = got.difference(&want).cloned().collect();
            let dropped: Vec<_> = want.difference(&got).cloned().collect();
            if !introduced.is_empty() {
                out.push(Issue {
                    file: file(loc),
                    message: format!(
                        "unknown placeholder in {k}: {{{}}}",
                        introduced.join("}, {")
                    ),
                });
            }
            // A plural form may spell its number out ("one card"): `{count}` may be dropped there.
            let dropped: Vec<_> = dropped
                .into_iter()
                .filter(|p| !(p == "count" && is_plural_key(k)))
                .collect();
            if !dropped.is_empty() {
                out.push(Issue {
                    file: file(loc),
                    message: format!("missing placeholder in {k}: {{{}}}", dropped.join("}, {")),
                });
            }
        }
        let lower = loc.to_lowercase().replace('_', "-");
        let mut sub = lower.split('-');
        let lang = sub.next().unwrap_or_default();
        let script = sub.find(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_alphabetic()));
        for (name, ranges, langs, scripts) in SCRIPTS {
            let allowed = match script {
                Some(s) => scripts.contains(&s),
                None => langs.contains(&lang),
            };
            if allowed {
                continue;
            }
            for (k, v) in map {
                if v.chars()
                    .any(|c| ranges.iter().any(|(a, b)| (*a..=*b).contains(&(c as u32))))
                {
                    out.push(Issue {
                        file: file(loc),
                        message: format!("{name} characters in {k}: \"{v}\""),
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_locale;

    #[test]
    fn finds_the_problems() {
        let mut a = Assets::new();
        a.insert("en".into(), parse_locale(r#"{"hi":"Hello {name}","n_one":"{count} card","n_other":"{count} cards","x":"Same","y":"Same","bad_one":"b"}"#).unwrap());
        a.insert("de".into(), parse_locale(r#"{"hi":"Hallo {nom}","n_one":"eine Karte","n_other":"{count} Karten","x":"Gleich","y":"Gleich","z":"Привет","bad_one":"b"}"#).unwrap());
        let used = vec![
            ("Hello {name}".to_string(), "app-ui/src/lib.rs".to_string()),
            ("Nope".to_string(), "app-ui/src/lib.rs".to_string()),
        ];
        let msgs: Vec<String> = check(&a, "en", &used)
            .into_iter()
            .map(|i| format!("{}: {}", i.file, i.message))
            .collect();
        let has = |s: &str| msgs.iter().any(|m| m.contains(s));
        assert!(
            has("plural family \"bad\" must define bad_other"),
            "{msgs:#?}"
        );
        assert!(has("app-ui/src/lib.rs: \"Nope\" is neither"), "{msgs:#?}");
        assert!(has("\"Same\" is both x and y"), "{msgs:#?}");
        let used_y = vec![("y".to_string(), "f".to_string())];
        assert!(
            !check(&a, "en", &used_y)
                .iter()
                .any(|i| i.message.contains("is both"))
        );
        assert!(has("locales/de.json: unknown key(s): z"), "{msgs:#?}");
        assert!(
            has("unknown placeholder in hi: {nom}") && has("missing placeholder in hi: {name}"),
            "{msgs:#?}"
        );
        assert!(!has("n_one"), "{msgs:#?}"); // `{count}` may be spelled out in a plural form
        assert!(has("Cyrillic characters in z"), "{msgs:#?}");
    }
}
