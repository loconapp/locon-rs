//! Matching the device's preferred languages to the app's locales, and writing direction.

struct Tag {
    language: String,
    script: Option<String>,
    region: Option<String>,
}

fn parse(tag: &str) -> Tag {
    let lower = tag.to_lowercase().replace('_', "-");
    let mut parts = lower.split('-');
    let language = parts.next().unwrap_or_default().to_string();
    let mut script = None;
    let mut region = None;
    for p in parts {
        if p.len() == 1 {
            break; // extensions (-u-, -x-)
        }
        if p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()) {
            script = Some(p.to_string());
        } else if (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
            || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
        {
            region = Some(p.to_string());
        }
    }
    // Chinese: the region implies the script when none is given.
    if language == "zh" && script.is_none() {
        script = match region.as_deref() {
            Some("tw" | "hk" | "mo") => Some("hant".into()),
            Some(_) => Some("hans".into()),
            None => None,
        };
    }
    Tag {
        language,
        script,
        region,
    }
}

/// The first of the device's preferred tags (in order) the app has: the same tag, else the same
/// language and script, else the same language. Returns the app's locale name as written.
pub fn match_locale(device: &[String], available: &[String]) -> Option<String> {
    let avail: Vec<(String, Tag)> = available.iter().map(|a| (a.clone(), parse(a))).collect();
    for d in device {
        let t = parse(d);
        let same = |a: &Tag| a.language == t.language;
        let exact = avail
            .iter()
            .find(|(_, a)| same(a) && a.script == t.script && a.region == t.region);
        let script = avail
            .iter()
            .find(|(_, a)| same(a) && t.script.is_some() && a.script == t.script);
        let plain = avail
            .iter()
            .find(|(_, a)| same(a) && a.script.is_none() && a.region.is_none());
        let any = avail
            .iter()
            .find(|(_, a)| same(a) && (a.script.is_none() || a.script == t.script));
        if let Some((name, _)) = exact.or(script).or(plain).or(any) {
            return Some(name.clone());
        }
    }
    None
}

/// Right-to-left locales (Arabic, Hebrew, Persian, Urdu, ... or an explicit RTL script).
pub fn is_rtl(locale: &str) -> bool {
    let t = parse(locale);
    if let Some(s) = t.script.as_deref() {
        return matches!(
            s,
            "arab" | "hebr" | "thaa" | "syrc" | "nkoo" | "adlm" | "rohg"
        );
    }
    matches!(
        t.language.as_str(),
        "ar" | "he"
            | "iw"
            | "fa"
            | "ur"
            | "ps"
            | "sd"
            | "ug"
            | "yi"
            | "ckb"
            | "dv"
            | "ku"
            | "azb"
            | "mzn"
            | "lrc"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        let av: Vec<String> = ["de", "en", "pt-BR", "zh-Hans", "zh-Hant"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m =
            |d: &[&str]| match_locale(&d.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &av);
        assert_eq!(m(&["de-AT"]).as_deref(), Some("de"));
        assert_eq!(m(&["fr", "en-US"]).as_deref(), Some("en"));
        assert_eq!(m(&["pt-PT"]).as_deref(), Some("pt-BR"));
        assert_eq!(m(&["zh-TW"]).as_deref(), Some("zh-Hant"));
        assert_eq!(m(&["zh-CN"]).as_deref(), Some("zh-Hans"));
        assert_eq!(m(&["ja"]), None);
        assert!(is_rtl("ar-EG") && is_rtl("he") && !is_rtl("de") && is_rtl("az-Arab"));
    }
}
