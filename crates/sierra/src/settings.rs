//! How a symbol's prices are stored: its real-time multiplier and tick
//! size, from Sierra Chart's symbol settings.
//!
//! Sierra Chart keeps one `SymbolSettings.<service>.xml` per data service
//! it has been connected to, each a few megabytes of `<settings>` blocks,
//! and lets `CustomSymbolSettings.<service>.xml` override any of them. A
//! block's `<symbol>` is usually a pattern, `ES?##_FUT_CME`, where `?`
//! stands for a month letter and `#` for a digit. The service in use is
//! recorded in `Sierra4.config`, a binary file this crate does not read;
//! instead the most recently modified service file that knows the symbol
//! wins, since that is the one Sierra Chart has been keeping up to date.
//! [`SymbolSettings::file`] says which one that was.
//!
//! The files are read with a scanner of their own rather than an XML
//! parser: only four elements matter, the format is Sierra Chart's own and
//! regular, and the whole file is scanned in a few milliseconds. What it
//! finds is kept by path and modification time, so asking again costs a
//! `stat` per file until Sierra Chart rewrites one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// How a symbol's stored prices become real ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// A stored price times this is the real price: 0.01 for ES, whose
    /// 7647.00 is stored as 764700.
    pub multiplier: f64,
    /// The smallest price step, in real prices: 0.25 for ES.
    pub tick_size: f64,
}

impl Scale {
    /// A stored price as a real one. When the multiplier is the
    /// reciprocal of a whole number, as it nearly always is, this divides
    /// by that number instead, so 7836 at 0.01 is exactly the nearest
    /// double to 78.36 rather than one a multiplication rounded off.
    pub fn real(&self, stored: f32) -> f64 {
        Scaler::new(self.multiplier).apply(stored)
    }
}

/// Scales stored prices, by division where that is exact.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Scaler {
    Multiply(f64),
    Divide(f64),
}

impl Scaler {
    pub(crate) fn new(multiplier: f64) -> Scaler {
        if multiplier > 0.0 && multiplier < 1.0 {
            let d = (1.0 / multiplier).round();
            if d <= 1e15 && (1.0 / d - multiplier).abs() <= multiplier * 1e-12 {
                return Scaler::Divide(d);
            }
        }
        Scaler::Multiply(multiplier)
    }

    #[inline(always)]
    pub(crate) fn apply(self, stored: f32) -> f64 {
        match self {
            Scaler::Multiply(m) => f64::from(stored) * m,
            Scaler::Divide(d) => f64::from(stored) / d,
        }
    }
}

/// What the settings say about a symbol, and where they said it.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolSettings {
    pub scale: Scale,
    /// The service file whose settings were used.
    pub file: PathBuf,
    /// The `<symbol>` that matched, a pattern such as `ES?##_FUT_CME`.
    pub symbol: String,
    /// The custom settings file that overrode something, if one did.
    pub custom: Option<PathBuf>,
}

/// One `<settings>` block, the parts of it that matter here.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Rule {
    pub symbol: String,
    pub pattern: bool,
    pub tick_size: Option<f64>,
    pub multiplier: Option<f64>,
}

/// A settings file, scanned.
#[derive(Debug, Default)]
pub(crate) struct Rules {
    rules: Vec<Rule>,
    /// Exact symbols, which win over any pattern.
    exact: HashMap<String, usize>,
}

impl Rules {
    pub(crate) fn scan(xml: &[u8]) -> Rules {
        let mut rules = Vec::new();
        let mut current: Option<Rule> = None;
        let mut at = 0;
        while let Some(off) = xml[at..].iter().position(|&b| b == b'<') {
            let start = at + off + 1;
            let Some(len) = xml[start..].iter().position(|&b| b == b'>') else { break };
            let tag = &xml[start..start + len];
            at = start + len + 1;
            let closing = tag.first() == Some(&b'/');
            let empty = tag.last() == Some(&b'/');
            let name = tag.strip_prefix(b"/").unwrap_or(tag);
            let name = name.strip_suffix(b"/").unwrap_or(name);
            let name = &name[..name.iter().position(u8::is_ascii_whitespace).unwrap_or(name.len())];
            if name == b"settings" {
                if closing {
                    rules.extend(current.take().filter(|r| !r.symbol.is_empty()));
                } else if !empty {
                    current = Some(Rule::default());
                }
                continue;
            }
            let Some(rule) = current.as_mut() else { continue };
            if closing || empty {
                continue;
            }
            let field = match name {
                b"symbol" => 0,
                b"tick-size" => 1,
                b"real-time-multiplier" => 2,
                b"use-pattern-matching-characters" => 3,
                _ => continue,
            };
            let text_len = xml[at..].iter().position(|&b| b == b'<').unwrap_or(xml.len() - at);
            let text = std::str::from_utf8(&xml[at..at + text_len]).unwrap_or("").trim();
            at += text_len;
            match field {
                0 => rule.symbol = unescape(text),
                1 => rule.tick_size = text.parse().ok(),
                2 => rule.multiplier = text.parse().ok(),
                _ => rule.pattern = text.eq_ignore_ascii_case("true") || text == "1",
            }
        }
        let mut exact = HashMap::new();
        for (i, r) in rules.iter().enumerate() {
            if !r.pattern || !r.symbol.bytes().any(|b| matches!(b, b'?' | b'#' | b'*')) {
                exact.entry(r.symbol.clone()).or_insert(i);
            }
        }
        Rules { rules, exact }
    }

    /// The rule for a symbol: an exact one if there is one, otherwise the
    /// first pattern that matches.
    pub(crate) fn find(&self, symbol: &str) -> Option<&Rule> {
        if let Some(&i) = self.exact.get(symbol) {
            return Some(&self.rules[i]);
        }
        self.rules.iter().find(|r| r.pattern && matches(r.symbol.as_bytes(), symbol.as_bytes()))
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.rules.len()
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Whether a Sierra Chart pattern matches a symbol: `?` is one letter,
/// `#` one digit, `*` any run of characters, anything else itself.
pub(crate) fn matches(pattern: &[u8], symbol: &[u8]) -> bool {
    match pattern.split_first() {
        None => symbol.is_empty(),
        Some((b'*', rest)) => (0..=symbol.len()).any(|i| matches(rest, &symbol[i..])),
        Some((&p, rest)) => {
            let Some((&s, tail)) = symbol.split_first() else { return false };
            let ok = match p {
                b'?' => s.is_ascii_alphabetic(),
                b'#' => s.is_ascii_digit(),
                _ => p == s,
            };
            ok && matches(rest, tail)
        }
    }
}

type Cache = Mutex<HashMap<PathBuf, (SystemTime, u64, Arc<Rules>)>>;

fn cache() -> &'static Cache {
    static CACHE: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// A settings file's rules, scanned once per version of the file.
pub(crate) fn rules(path: &Path) -> Option<Arc<Rules>> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    if let Some((m, len, rules)) = cache().lock().unwrap().get(path)
        && *m == modified
        && *len == meta.len()
    {
        return Some(rules.clone());
    }
    let rules = Arc::new(Rules::scan(&std::fs::read(path).ok()?));
    cache().lock().unwrap().insert(path.to_path_buf(), (modified, meta.len(), rules.clone()));
    Some(rules)
}

/// Looks a symbol up in a `SymbolSettings` folder, as described for the
/// module.
pub fn lookup(folder: &Path, symbol: &str) -> Option<SymbolSettings> {
    let mut services: Vec<(SystemTime, String, PathBuf)> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let service = name.strip_prefix("SymbolSettings.")?.strip_suffix(".xml")?.to_owned();
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, service, e.path()))
        })
        .collect();
    services.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, service, path) in services {
        let custom_path = folder.join(format!("CustomSymbolSettings.{service}.xml"));
        let base = rules(&path);
        let custom = rules(&custom_path);
        let base = base.as_deref().and_then(|r| r.find(symbol));
        let over = custom.as_deref().and_then(|r| r.find(symbol));
        let Some(matched) = base.or(over) else { continue };
        let pick = |f: fn(&Rule) -> Option<f64>| over.and_then(f).map(|v| (v, true)).or_else(|| base.and_then(f).map(|v| (v, false)));
        let Some((tick_size, tick_over)) = pick(|r| r.tick_size) else { continue };
        let (multiplier, mult_over) = pick(|r| r.multiplier).unwrap_or((1.0, false));
        if !(tick_size > 0.0 && multiplier > 0.0) {
            continue;
        }
        return Some(SymbolSettings {
            scale: Scale { multiplier, tick_size },
            file: path,
            symbol: matched.symbol.clone(),
            custom: (tick_over || mult_over).then_some(custom_path),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;

    fn block(symbol: &str, tick: &str, mult: &str, pattern: bool) -> String {
        format!(
            "    <settings>\n        <symbol>{symbol}</symbol>\n        <description>A &amp; B</description>\n        <tick-size>{tick}</tick-size>\n        <use-pattern-matching-characters>{pattern}</use-pattern-matching-characters>\n        <real-time-multiplier>{mult}</real-time-multiplier>\n    </settings>\n"
        )
    }

    fn file(blocks: &[String]) -> String {
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<symbol-settings>\n{}</symbol-settings>\n", blocks.concat())
    }

    #[test]
    fn patterns_match_month_letters_and_digits() {
        assert!(matches(b"ES?##_FUT_CME", b"ESZ26_FUT_CME"));
        assert!(!matches(b"ES?##_FUT_CME", b"ESZ6_FUT_CME"));
        assert!(!matches(b"ES?##_FUT_CME", b"ES126_FUT_CME"));
        assert!(!matches(b"ES?##_FUT_CME", b"NQZ26_FUT_CME"));
        assert!(matches(b"ES*", b"ESZ26_FUT_CME"));
        assert!(matches(b"*_CME", b"ESZ26_FUT_CME"));
    }

    #[test]
    fn a_settings_file_is_scanned_for_its_symbols_and_values() {
        let xml = file(&[
            "<settings>\n<symbol-srvcode/>\n<symbol/>\n<is-search-pattern/>\n</settings>\n".to_owned(),
            block("ES?##_FUT_CME", "0.25", ".01", true),
            block("GC?##_FUT_CME", "0.1", "0.1", true),
            block("EURUSD", "0.00001", "1", false),
        ]);
        let rules = Rules::scan(xml.as_bytes());
        assert_eq!(rules.len(), 3);
        let es = rules.find("ESZ26_FUT_CME").unwrap();
        assert_eq!((es.tick_size, es.multiplier), (Some(0.25), Some(0.01)));
        assert_eq!(rules.find("GCZ26_FUT_CME").unwrap().multiplier, Some(0.1));
        assert_eq!(rules.find("EURUSD").unwrap().tick_size, Some(0.00001));
        assert!(rules.find("NQZ26_FUT_CME").is_none());
        assert!(rules.find("").is_none());
    }

    #[test]
    fn an_exact_symbol_wins_over_a_pattern_and_patterns_are_literal_when_switched_off() {
        let xml = file(&[block("ES?##_FUT_CME", "0.25", ".01", true), block("ESZ26_FUT_CME", "0.5", "1", false), block("NQ?##_FUT_CME", "0.25", ".01", false)]);
        let rules = Rules::scan(xml.as_bytes());
        assert_eq!(rules.find("ESZ26_FUT_CME").unwrap().tick_size, Some(0.5));
        assert_eq!(rules.find("ESH27_FUT_CME").unwrap().tick_size, Some(0.25));
        assert!(rules.find("NQZ26_FUT_CME").is_none());
        assert!(rules.find("NQ?##_FUT_CME").is_some());
    }

    #[test]
    fn the_newest_service_that_knows_a_symbol_wins_and_its_custom_file_overrides_it() {
        let dir = Scratch::new();
        let old = dir.join("SymbolSettings.rithmic_v2.trading.xml");
        let new = dir.join("SymbolSettings.sc_futures_direct.dtc.trading.xml");
        std::fs::write(&old, file(&[block("ES?##_FUT_CME", "0.25", "1", true), block("CL?##_FUT_CME", "0.01", ".01", true)])).unwrap();
        std::fs::write(&new, file(&[block("ES?##_FUT_CME", "0.25", ".01", true)])).unwrap();
        let past = SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(past).unwrap();

        let es = lookup(dir.path(), "ESZ26_FUT_CME").unwrap();
        assert_eq!(es.scale, Scale { multiplier: 0.01, tick_size: 0.25 });
        assert_eq!((es.file.as_path(), es.symbol.as_str(), es.custom.as_ref()), (new.as_path(), "ES?##_FUT_CME", None));
        let cl = lookup(dir.path(), "CLZ26_FUT_CME").unwrap();
        assert_eq!(cl.file, old);
        assert!(lookup(dir.path(), "ZZZ").is_none());

        let custom = dir.join("CustomSymbolSettings.sc_futures_direct.dtc.trading.xml");
        std::fs::write(&custom, "<symbol-settings>\n\t<settings>\n\t\t<symbol>ES?##_FUT_CME</symbol>\n\t\t<use-pattern-matching-characters>true</use-pattern-matching-characters>\n\t\t<tick-size>0.5</tick-size>\n\t</settings>\n</symbol-settings>\n").unwrap();
        let es = lookup(dir.path(), "ESZ26_FUT_CME").unwrap();
        assert_eq!(es.scale, Scale { multiplier: 0.01, tick_size: 0.5 });
        assert_eq!(es.custom, Some(custom));
    }

    #[test]
    fn a_settings_file_rewritten_is_scanned_again() {
        let dir = Scratch::new();
        let path = dir.join("SymbolSettings.x.xml");
        std::fs::write(&path, file(&[block("ES?##_FUT_CME", "0.25", ".01", true)])).unwrap();
        assert_eq!(lookup(dir.path(), "ESZ26_FUT_CME").unwrap().scale.multiplier, 0.01);
        std::fs::write(&path, file(&[block("ES?##_FUT_CME", "0.25", "0.10", true)])).unwrap();
        assert_eq!(lookup(dir.path(), "ESZ26_FUT_CME").unwrap().scale.multiplier, 0.1);
    }

    #[test]
    fn stored_prices_scale_to_the_real_ones() {
        let scale = |m, p: f32| Scale { multiplier: m, tick_size: 0.01 }.real(p);
        assert_eq!(scale(0.01, 764_700.0), 7647.0);
        assert_eq!(scale(0.01, 7836.0), 78.36);
        assert_eq!(scale(0.1, 41_184.0), 4118.4);
        assert_eq!(scale(1.0, 1.5), 1.5);
        assert_eq!(scale(0.0001, 12_414.0), 1.2414);
    }
}
