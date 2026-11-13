//! Formulas: arithmetic on symbols. `GC/SI` is gold over silver, `TNX-FVX`
//! the ten-year less the five-year, `(2*RB+HO)*42/3-CL` the 3-2-1 crack,
//! `(SPY+QQQ)/2` a basket, and `USDEUR` one over the listed `EURUSD`.
//!
//! Nothing trades a formula, so no provider serves one. It is a fetch per
//! leg and the arithmetic, done here rather than in each provider: a
//! provider is asked for each leg in its own symbology, exactly as it would
//! be for a chart of it, and never learns what was going to be done with it.
//!
//! The legs rarely print on the same clock. Gold trades nearly around the
//! clock and a miners' ETF six and a half hours a day; Yahoo stamps a
//! future's daily bar at midnight New York and a stock's at the open. So the
//! first leg is the clock, and every other leg is joined *as of* each of its
//! bars — the latest one at or before it — which is what a chart of the
//! formula would show if somebody worked it out from the quotes on their
//! screen at that moment.
//!
//! Spelled the way people type it: `+ - * /`, numbers and parentheses, with
//! the usual precedence. A dash is also part of real tickers — `BTC-USD`,
//! `BRK-B` — so a run of characters with a dash in it is a symbol
//! when it names one, and a subtraction only when it does not: `SPY-QQQ` is
//! a spread, `BTC-USD` is Bitcoin, and `BTC-USD - ETH-USD` is both.

use serde::{Deserialize, Serialize};

use crate::bars::{Bar, Timeframe};
use crate::compare::moment;
use crate::provider::{Provider, ProviderError};
use crate::symbols::Instrument;

/// What a leg is wrapped in, in a provider symbol: `{GC=F}/{SI=F}`. Nothing
/// any provider spells a symbol with — Yahoo has `^`, `=` and `-`, and
/// thinkorswim a leading `/` — so a leg can be anything a provider calls it.
pub const LEG_OPEN: char = '{';
const LEG_CLOSE: char = '}';

/// How far back a leg after the first is asked for beyond the first, so the
/// first bar of a fetch still has something to be joined to across a
/// weekend and a holiday.
const LOOKBACK: i64 = 7 * 86_400;

/// How deep parentheses may nest. Far past anything anybody types, and short
/// of a stack overflow, which in a command would take the window down.
const DEPTH: usize = 32;

/// Several symbols and the arithmetic done on them.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Formula {
    /// Each symbol it reads, once, in the order they were first spelled.
    /// The first is the clock the others are joined to. None is a formula
    /// itself: one spelled with another in it, `USDEUR/GC`, carries the
    /// inner one's legs.
    pub legs: Vec<Instrument>,
    pub expr: Expr,
}

impl Formula {
    /// What a provider is asked for: the formula with each leg in the
    /// provider's own spelling, `{GC=F}/{SI=F}`. `None` when one of the legs
    /// is something the provider does not serve.
    pub fn provider_symbol(&self, native: impl Fn(&Instrument) -> Option<String>) -> Option<String> {
        let natives = self.legs.iter().map(native).collect::<Option<Vec<_>>>()?;
        // A formula is read back out of a stored instrument, which anything
        // can write, so a leg past the end is refused rather than spelled.
        if self.expr.any_leg(&|at| at >= natives.len()) {
            return None;
        }
        let leg = |at: usize| format!("{LEG_OPEN}{}{LEG_CLOSE}", natives.get(at).map_or("", String::as_str));
        Some(self.expr.spell(&leg, &TYPED))
    }

    /// Whether a move in it is read in its own units rather than in percent.
    /// Anything that subtracts can cross zero, and a spread that does has
    /// moved an infinite percentage. A sum or a scaling of rates is still a
    /// rate. A product or a quotient of two legs is a ratio, read in percent
    /// whatever its legs are.
    pub fn in_units(&self) -> bool {
        self.expr.subtracts() || (!self.expr.combines_legs() && self.legs.iter().all(Instrument::moves_in_units))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    fn from_char(c: char) -> Option<Op> {
        Some(match c {
            '+' => Op::Add,
            '-' => Op::Sub,
            '*' => Op::Mul,
            '/' => Op::Div,
            _ => return None,
        })
    }

    fn precedence(self) -> u8 {
        match self {
            Op::Add | Op::Sub => 1,
            Op::Mul | Op::Div => 2,
        }
    }

    fn apply(self, a: f64, b: f64) -> Option<f64> {
        Some(match self {
            Op::Add => a + b,
            Op::Sub => a - b,
            Op::Mul => a * b,
            Op::Div if b == 0.0 => return None,
            Op::Div => a / b,
        })
    }
}

/// The arithmetic, with each leg named by its place in [`Formula::legs`].
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Expr {
    Num(f64),
    Leg(usize),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

// Never NaN: a number is only ever one that was spelled in digits.
impl Eq for Expr {}

impl Expr {
    /// Its value with each leg at `leg`. `None` for a division by zero, or
    /// anything else that is not a number.
    pub fn eval(&self, leg: &dyn Fn(usize) -> f64) -> Option<f64> {
        let value = match self {
            Expr::Num(n) => *n,
            Expr::Leg(at) => leg(*at),
            Expr::Neg(inner) => -inner.eval(leg)?,
            Expr::Bin(op, a, b) => op.apply(a.eval(leg)?, b.eval(leg)?)?,
        };
        Some(value).filter(|v| v.is_finite())
    }

    /// Whether any leg it names, by its place in [`Formula::legs`], is one
    /// `which` picks.
    fn any_leg(&self, which: &dyn Fn(usize) -> bool) -> bool {
        match self {
            Expr::Num(_) => false,
            Expr::Leg(at) => which(*at),
            Expr::Neg(inner) => inner.any_leg(which),
            Expr::Bin(_, a, b) => a.any_leg(which) || b.any_leg(which),
        }
    }

    fn subtracts(&self) -> bool {
        match self {
            Expr::Num(_) | Expr::Leg(_) => false,
            Expr::Neg(_) | Expr::Bin(Op::Sub, ..) => true,
            Expr::Bin(_, a, b) => a.subtracts() || b.subtracts(),
        }
    }

    /// Whether two legs are multiplied or divided: `GC/SI`, not `2*GC`.
    fn combines_legs(&self) -> bool {
        match self {
            Expr::Num(_) | Expr::Leg(_) => false,
            Expr::Neg(inner) => inner.combines_legs(),
            Expr::Bin(Op::Mul | Op::Div, a, b) if a.any_leg(&|_| true) && b.any_leg(&|_| true) => true,
            Expr::Bin(_, a, b) => a.combines_legs() || b.combines_legs(),
        }
    }

    /// Each leg replaced by what `with` makes of it.
    pub fn substitute(&self, with: &dyn Fn(usize) -> Expr) -> Expr {
        match self {
            Expr::Num(n) => Expr::Num(*n),
            Expr::Leg(at) => with(*at),
            Expr::Neg(inner) => Expr::Neg(Box::new(inner.substitute(with))),
            Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(a.substitute(with)), Box::new(b.substitute(with))),
        }
    }

    /// Written out, with no more parentheses than its precedence needs.
    /// `min` is the precedence the surroundings require of it.
    fn write(&self, out: &mut String, leg: &dyn Fn(usize) -> String, style: &Style, min: u8) {
        match self {
            Expr::Num(n) => out.push_str(&n.to_string()),
            Expr::Leg(at) => out.push_str(&leg(*at)),
            Expr::Neg(inner) => {
                out.push_str(style.neg);
                inner.write(out, leg, style, 3);
            }
            Expr::Bin(op, a, b) => {
                let own = op.precedence();
                let wrap = own < min;
                if wrap {
                    out.push('(');
                }
                a.write(out, leg, style, own);
                out.push_str(style.ops[*op as usize]);
                // Left to right: `A-(B-C)` keeps its parentheses, and
                // `(A-B)-C` needs none.
                b.write(out, leg, style, own + 1);
                if wrap {
                    out.push(')');
                }
            }
        }
    }

    /// Written out with each leg as `leg` spells it, in `style`.
    pub fn spell(&self, leg: &dyn Fn(usize) -> String, style: &Style) -> String {
        let mut out = String::new();
        self.write(&mut out, leg, style, 0);
        out
    }
}

/// How the four operators and a minus sign are written, in [`Op`]'s order.
pub struct Style {
    ops: [&'static str; 4],
    neg: &'static str,
}

/// As it is typed: `3*CL-HO`.
pub const TYPED: Style = Style { ops: ["+", "-", "*", "/"], neg: "-" };

/// With room round adding and subtracting, `BRK - B`, for when the typed
/// spelling would read back as one symbol.
pub const SPACED: Style = Style { ops: [" + ", " - ", "*", "/"], neg: "-" };

/// As it reads in a name: `Gold / Silver`, `3 × Crude Oil − Heating Oil`.
pub const NAMED: Style = Style { ops: [" + ", " − ", " × ", " / "], neg: "−" };

#[derive(Clone, Copy, PartialEq, Debug)]
enum Token {
    Num(f64),
    Leg(usize),
    Op(Op),
    Open,
    Close,
}

/// What a ticker can be spelled with, a dash aside. `=` and `^` are Yahoo's.
fn symbolic(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '=' | '^' | '_' | '&')
}

/// A run of digits with at most one point, `42` or `0.5`. Never a ticker:
/// one that is all digits, like `2330`, always carries its venue.
fn number(text: &str) -> Option<f64> {
    let digits = text.chars().all(|c| c.is_ascii_digit() || c == '.');
    match digits && text.chars().any(|c| c.is_ascii_digit()) && text.matches('.').count() <= 1 {
        true => text.parse().ok(),
        false => None,
    }
}

/// A formula as somebody typed it, with each symbol resolved by `resolve`.
///
/// The legs come back in the order they were first spelled, each once: `GC`
/// twice in a formula is one leg. `None` for anything that does not parse,
/// a symbol that does not resolve, or no symbol at all — `2+2` is nothing
/// to chart.
pub fn parse(text: &str, resolve: impl Fn(&str) -> Option<Instrument>) -> Option<(Expr, Vec<Instrument>)> {
    let mut legs: Vec<Instrument> = Vec::new();
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        at += 1;
        match c {
            c if c.is_whitespace() => {}
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            c if symbolic(c) => {
                let (run, end) = symbol_run(&chars, at - 1);
                at = end;
                let pieces: Vec<&str> = run.split('-').collect();
                for (i, atom) in split(&pieces, &resolve)?.into_iter().enumerate() {
                    if i > 0 {
                        tokens.push(Token::Op(Op::Sub));
                    }
                    tokens.push(match atom {
                        Atom::Num(n) => Token::Num(n),
                        Atom::Symbol(found) => {
                            Token::Leg(intern(&mut legs, found, same_leg))
                        }
                    });
                }
            }
            c => tokens.push(Token::Op(Op::from_char(c)?)),
        }
    }
    let expr = Parser::new(&tokens).whole()?;
    expr.any_leg(&|_| true).then_some((expr, legs))
}

/// The run of symbol characters starting at `start`, and where it ends. A
/// dash belongs to the run only between two characters that do:
/// `BTC-USD`, not the minus of `SPY -QQQ`.
fn symbol_run(chars: &[char], start: usize) -> (String, usize) {
    let mut at = start;
    while let Some(&c) = chars.get(at) {
        let inner = c == '-' && at > start && chars.get(at + 1).copied().is_some_and(symbolic);
        if !symbolic(c) && !inner {
            break;
        }
        at += 1;
    }
    (chars[start..at].iter().collect(), at)
}

/// Whether two legs are the same listing: `GC` twice in a formula is one leg.
pub(crate) fn same_leg(a: &Instrument, b: &Instrument) -> bool {
    a.symbol == b.symbol && a.suffix == b.suffix
}

/// Where `item` is in `list` by `same`, put at its end the first time:
/// a leg spelled twice is one leg.
pub(crate) fn intern<T>(list: &mut Vec<T>, item: T, same: impl Fn(&T, &T) -> bool) -> usize {
    match list.iter().position(|had| same(had, &item)) {
        Some(at) => at,
        None => {
            list.push(item);
            list.len() - 1
        }
    }
}

enum Atom {
    Num(f64),
    Symbol(Instrument),
}

/// A run split at its dashes into things that each resolve: the longest
/// first, so a run that names a symbol whole is that symbol.
fn split(pieces: &[&str], resolve: &impl Fn(&str) -> Option<Instrument>) -> Option<Vec<Atom>> {
    if pieces.is_empty() {
        return Some(Vec::new());
    }
    for end in (1..=pieces.len()).rev() {
        let text = pieces[..end].join("-");
        let atom = match number(&text) {
            Some(n) => Atom::Num(n),
            None => match resolve(&text) {
                Some(found) => Atom::Symbol(found),
                None => continue,
            },
        };
        if let Some(mut rest) = split(&pieces[end..], resolve) {
            rest.insert(0, atom);
            return Some(rest);
        }
    }
    None
}

/// A provider symbol, `{GC=F}/{SI=F}`, read back into its arithmetic and
/// the provider's spelling of each leg.
fn read(symbol: &str) -> Option<(Expr, Vec<String>)> {
    let mut natives: Vec<String> = Vec::new();
    let mut tokens = Vec::new();
    let mut rest = symbol;
    while let Some(c) = rest.chars().next() {
        if let Some(inner) = rest.strip_prefix(LEG_OPEN) {
            let (native, after) = inner.split_once(LEG_CLOSE)?;
            tokens.push(Token::Leg(intern(&mut natives, native.to_string(), |a, b| a == b)));
            rest = after;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
            tokens.push(Token::Num(number(&rest[..end])?));
            rest = &rest[end..];
            continue;
        }
        tokens.push(match c {
            '(' => Token::Open,
            ')' => Token::Close,
            c => Token::Op(Op::from_char(c)?),
        });
        rest = &rest[c.len_utf8()..];
    }
    Some((Parser::new(&tokens).whole()?, natives))
}

/// The usual precedence, read left to right: `*` and `/` before `+` and `-`,
/// and a minus in front of anything negates it.
struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Parser { tokens, at: 0, depth: 0 }
    }

    fn whole(mut self) -> Option<Expr> {
        let expr = self.sum()?;
        (self.at == self.tokens.len()).then_some(expr)
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn sum(&mut self) -> Option<Expr> {
        let mut left = self.product()?;
        while let Some(Token::Op(op @ (Op::Add | Op::Sub))) = self.peek() {
            self.at += 1;
            left = Expr::Bin(op, Box::new(left), Box::new(self.product()?));
        }
        Some(left)
    }

    fn product(&mut self) -> Option<Expr> {
        let mut left = self.unary()?;
        while let Some(Token::Op(op @ (Op::Mul | Op::Div))) = self.peek() {
            self.at += 1;
            left = Expr::Bin(op, Box::new(left), Box::new(self.unary()?));
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Expr> {
        self.depth += 1;
        if self.depth > DEPTH {
            return None;
        }
        let expr = match self.peek()? {
            Token::Op(Op::Sub) => {
                self.at += 1;
                Expr::Neg(Box::new(self.unary()?))
            }
            Token::Op(Op::Add) => {
                self.at += 1;
                self.unary()?
            }
            Token::Num(n) => {
                self.at += 1;
                Expr::Num(n)
            }
            Token::Leg(at) => {
                self.at += 1;
                Expr::Leg(at)
            }
            Token::Open => {
                self.at += 1;
                let inner = self.sum()?;
                if self.peek()? != Token::Close {
                    return None;
                }
                self.at += 1;
                inner
            }
            Token::Op(_) | Token::Close => return None,
        };
        self.depth -= 1;
        Some(expr)
    }
}

/// Whether a provider symbol is a formula rather than one thing a provider
/// serves.
pub fn is_formula(symbol: &str) -> bool {
    symbol.contains(LEG_OPEN)
}

/// Whether typed text can only be a formula: it has an operator in it that
/// no ticker is spelled with. A dash alone could be either, `BTC-USD`.
pub fn looks_like(text: &str) -> bool {
    text.contains(['/', '*', '+', '(', ')'])
}

/// [`Provider::bars`], for a formula as well as for anything that trades.
///
/// A symbol that is not a formula goes straight to the provider. One that
/// is costs a request per leg, back to back: the loader's pacing sees one,
/// which for a polled feed with a generous limit is a rounding error, and is
/// the price of keeping formulas out of every provider.
pub fn bars(
    provider: &dyn Provider,
    symbol: &str,
    timeframe: Timeframe,
    since: Option<i64>,
) -> Result<Vec<Bar>, ProviderError> {
    if !is_formula(symbol) {
        return provider.bars(symbol, timeframe, since);
    }
    let Some((expr, natives)) = read(symbol) else {
        return Err(ProviderError::NotFound);
    };
    let mut series = Vec::with_capacity(natives.len());
    for (at, native) in natives.iter().enumerate() {
        let from = match at {
            0 => since,
            _ => since.map(|s| s - LOOKBACK),
        };
        let bars = provider.bars(native, timeframe, from)?;
        if at == 0 && bars.is_empty() {
            return Ok(bars);
        }
        series.push(bars);
    }
    Ok(combine(&expr, &series, timeframe))
}

/// The formula worked out on each bar of the first leg, with every other leg
/// at the bar in force when it printed.
///
/// Open on the opens and close on the closes. The high on the highs is not
/// the formula's high — the legs need not have peaked together, and a leg
/// that is subtracted or divided by pushes the other way — so the high and
/// low are the larger and smaller of the formula on the highs and on the
/// lows, widened only as far as the open and close need to sit inside them.
///
/// A bar with a leg missing, or only one too old to speak for it, is left
/// out rather than worked out from a stale price, and so is one that divides
/// by zero. Too old is a week, or two bars of the chart's resolution when
/// those are longer.
pub fn combine(expr: &Expr, series: &[Vec<Bar>], timeframe: Timeframe) -> Vec<Bar> {
    let Some((clock, others)) = series.split_first() else { return Vec::new() };
    let stale_after = LOOKBACK.max(2 * timeframe.seconds());
    let by_day = timeframe.seconds() >= 86_400;
    let mut next = vec![0; others.len()];
    let mut current: Vec<Option<Bar>> = vec![None; others.len()];
    let mut legs: Vec<Bar> = Vec::with_capacity(series.len());
    let mut out = Vec::with_capacity(clock.len());
    'bars: for bar in clock {
        let at = moment(bar.ts, by_day);
        legs.clear();
        legs.push(*bar);
        for (k, other) in others.iter().enumerate() {
            while let Some(candidate) = other.get(next[k]) {
                if moment(candidate.ts, by_day) > at {
                    break;
                }
                current[k] = Some(*candidate);
                next[k] += 1;
            }
            let Some(under) = current[k] else { continue 'bars };
            if bar.ts - under.ts > stale_after {
                continue 'bars;
            }
            legs.push(under);
        }
        let value = |pick: fn(&Bar) -> f64| expr.eval(&|i| legs.get(i).map_or(f64::NAN, pick));
        let (Some(open), Some(high), Some(low), Some(close)) =
            (value(|b| b.open), value(|b| b.high), value(|b| b.low), value(|b| b.close))
        else {
            continue;
        };
        out.push(Bar {
            ts: bar.ts,
            open,
            high: high.max(low).max(open).max(close),
            low: high.min(low).min(open).min(close),
            close,
            volume: 0.0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn bar(ts: i64, open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar { ts, open, high, low, close, volume: 100.0 }
    }

    fn flat(ts: i64, price: f64) -> Bar {
        bar(ts, price, price, price, price)
    }

    fn over() -> Expr {
        Expr::Bin(Op::Div, Box::new(Expr::Leg(0)), Box::new(Expr::Leg(1)))
    }

    fn divide(top: &[Bar], bottom: &[Bar], timeframe: Timeframe) -> Vec<Bar> {
        combine(&over(), &[top.to_vec(), bottom.to_vec()], timeframe)
    }

    /// A stand-in for the index: a symbol is anything in capitals with no
    /// dash, plus the few dashed ones listed.
    fn known(text: &str) -> Option<Instrument> {
        let dashed = ["BTC-USD", "ETH-USD", "BRK-B", "CL-1"];
        let plain = !text.is_empty() && text.chars().all(|c| c.is_ascii_uppercase() || c == '.' || c == ':');
        (plain || dashed.contains(&text)).then(|| {
            let mut found = crate::symbols::seed().into_iter().next().expect("a seeded row");
            found.symbol = text.to_string();
            found.suffix = None;
            found
        })
    }

    fn spelled(text: &str) -> Option<String> {
        let (expr, legs) = parse(text, known)?;
        Some(expr.spell(&|at| legs[at].symbol.clone(), &TYPED))
    }

    #[test]
    fn arithmetic_reads_with_the_usual_precedence() {
        assert_eq!(spelled("GC/SI").as_deref(), Some("GC/SI"));
        assert_eq!(spelled("3 * CL - 2*RB - HO").as_deref(), Some("3*CL-2*RB-HO"));
        assert_eq!(spelled("(SPY+QQQ)/2").as_deref(), Some("(SPY+QQQ)/2"));
        assert_eq!(spelled("SPY+QQQ/2").as_deref(), Some("SPY+QQQ/2"));
        assert_eq!(spelled("A-(B-C)").as_deref(), Some("A-(B-C)"));
        assert_eq!(spelled("(A-B)-C").as_deref(), Some("A-B-C"));
        assert_eq!(spelled("-TNX").as_deref(), Some("-TNX"));
        assert_eq!(spelled("1/EURUSD").as_deref(), Some("1/EURUSD"));
        assert_eq!(spelled("HYG/LQD*100").as_deref(), Some("HYG/LQD*100"));
        for nothing in ["", "2+2", "GC/", "(GC", "GC)", "GC//SI", "GC SI", "GC%SI"] {
            assert_eq!(spelled(nothing), None, "{nothing:?}");
        }
        let deep = format!("{}GC{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(spelled(&deep), None, "nested past any sense, refused rather than overflowed");
    }

    #[test]
    fn a_dash_is_a_symbol_first_and_a_minus_otherwise() {
        assert_eq!(spelled("SPY-QQQ").as_deref(), Some("SPY-QQQ"));
        let (expr, legs) = parse("BTC-USD", known).expect("bitcoin");
        assert_eq!((expr, legs.len()), (Expr::Leg(0), 1), "one symbol, not BTC minus USD");
        let (_, legs) = parse("BTC-USD-ETH-USD", known).expect("two pairs");
        let names: Vec<&str> = legs.iter().map(|l| l.symbol.as_str()).collect();
        assert_eq!(names, ["BTC-USD", "ETH-USD"]);
        let (_, legs) = parse("BRK-B - SPY", known).expect("spaced");
        assert_eq!(legs[0].symbol, "BRK-B");
        assert_eq!(spelled("CL-1").as_deref(), Some("CL-1"), "a futures spelling stays one");
        assert_eq!(spelled("CL - 1").as_deref(), Some("CL-1"), "spaced, it subtracts");
        let (expr, _) = parse("CL - 1", known).expect("crude less a dollar");
        assert!(matches!(expr, Expr::Bin(Op::Sub, ..)));
    }

    /// Venue and contract marks are part of a symbol, and a dash is part
    /// of one only between two of its characters.
    #[test]
    fn a_symbol_runs_through_its_marks() {
        let any = |text: &str| {
            let dashed = ["CL-2", "BTC-USD"];
            (!text.contains('-') || dashed.contains(&text)).then(|| known("X")).flatten().map(|mut found| {
                found.symbol = text.to_string();
                found
            })
        };
        let legs = |text: &str| parse(text, any).map(|(_, legs)| legs.into_iter().map(|l| l.symbol).collect::<Vec<_>>());
        assert_eq!(legs("GC=F/SI=F").unwrap(), ["GC=F", "SI=F"]);
        assert_eq!(legs("^TNX-^FVX").unwrap(), ["^TNX", "^FVX"], "no such pair, so a spread");
        // The trap: tight, `CL-2` is the second contract, not crude less two.
        assert_eq!(legs("3*CL-2*RB").unwrap(), ["CL-2", "RB"]);
        assert_eq!(legs("3*CL - 2*RB").unwrap(), ["CL", "RB"]);
        assert_eq!(legs("SPY -QQQ").unwrap(), ["SPY", "QQQ"], "a dash before a gap is a minus");
        assert_eq!(legs("BTC-USD-SPY").unwrap(), ["BTC-USD", "SPY"]);
        assert_eq!(legs("BTC-USD/BTC-USD").unwrap(), ["BTC-USD"]);
        let (expr, _) = parse("CL - -1", any).expect("less a negative");
        assert!(matches!(expr, Expr::Bin(Op::Sub, _, ref b) if matches!(**b, Expr::Neg(_))));
        for nothing in ["BTC-", "-", "GC€", "CL--"] {
            assert_eq!(legs(nothing), None, "{nothing:?}");
        }
    }

    #[test]
    fn a_symbol_spelled_twice_is_one_leg() {
        let (expr, legs) = parse("GC/(GC+SI)", known).expect("gold's share");
        assert_eq!(legs.len(), 2);
        assert_eq!(expr.eval(&|at| [3.0, 1.0][at]), Some(0.75));
    }

    #[test]
    fn a_provider_symbol_reads_back_into_the_same_arithmetic() {
        let (expr, legs) = parse("3*CL-2*RB*42-(HO+1)/2", known).expect("a crack spread");
        let formula = Formula { legs, expr: expr.clone() };
        let native = formula.provider_symbol(|leg| Some(format!("{}=F", leg.symbol))).expect("all served");
        assert_eq!(native, "3*{CL=F}-2*{RB=F}*42-({HO=F}+1)/2");
        let (back, natives) = read(&native).expect("reads back");
        assert_eq!(back, expr);
        assert_eq!(natives, ["CL=F", "RB=F", "HO=F"]);
        assert_eq!(formula.provider_symbol(|_| None), None, "a leg the provider lacks is no formula");
        assert!(is_formula(&native));
        assert!(!is_formula("BTC-USD"));
    }

    #[test]
    fn open_and_close_divide_and_the_range_holds_them() {
        let top = [bar(0, 100.0, 120.0, 90.0, 110.0)];
        let bottom = [bar(0, 50.0, 60.0, 40.0, 44.0)];
        let out = divide(&top, &bottom, Timeframe::minutes(5));
        assert_eq!(out.len(), 1);
        let r = out[0];
        assert_eq!((r.open, r.close), (2.0, 2.5));
        assert_eq!(r.high, 2.5, "120/60 and 90/40 are under the close, so the close is the high");
        assert_eq!(r.low, 2.0, "both are over the open, so the open is the low");
        assert_eq!(r.volume, 0.0, "a formula has no volume");
    }

    /// The ten-year less the three-month: a spread goes negative, and the
    /// leg that is subtracted pulls its high down.
    #[test]
    fn a_spread_can_go_below_zero() {
        let spread = Expr::Bin(Op::Sub, Box::new(Expr::Leg(0)), Box::new(Expr::Leg(1)));
        let ten = [bar(0, 4.0, 4.2, 3.9, 4.1)];
        let three = [bar(0, 4.5, 4.6, 4.4, 4.5)];
        let out = combine(&spread, &[ten.to_vec(), three.to_vec()], Timeframe::days(1));
        let r = out[0];
        assert!((r.open + 0.5).abs() < 1e-9 && (r.close + 0.4).abs() < 1e-9);
        assert!(r.low <= r.open.min(r.close) && r.high >= r.open.max(r.close));
    }

    /// A miners' ETF over gold, hourly: the ETF prints on the half hour and
    /// gold on the hour, and each ETF bar goes over the gold bar it sits in.
    #[test]
    fn the_other_legs_are_the_ones_in_force() {
        let top = [flat(1_800, 30.0), flat(5_400, 33.0)];
        let bottom = [flat(0, 3.0), flat(3_600, 3.3), flat(7_200, 99.0)];
        let out = divide(&top, &bottom, Timeframe::hours(1));
        let closes: Vec<f64> = out.iter().map(|b| b.close).collect();
        assert_eq!(closes, [10.0, 10.0]);
        assert_eq!(out[0].ts, 1_800, "the first leg's clock is the formula's");
    }

    /// Yahoo's daily stamps: a future at 04:00 UTC, a stock at 13:30, FX at
    /// 23:00 the evening before. All the same trading day.
    #[test]
    fn daily_bars_meet_on_their_date_whatever_hour_they_carry() {
        let day = 20_000 * DAY;
        let top = [flat(day + 13 * 3_600 + 1_800, 50.0)];
        for stamp in [day + 4 * 3_600, day - 3_600, day] {
            let out = divide(&top, &[flat(stamp, 25.0)], Timeframe::days(1));
            assert_eq!(out.len(), 1, "stamp {stamp} missed");
            assert_eq!(out[0].close, 2.0);
        }
        let tomorrow = [flat(day + DAY + 4 * 3_600, 25.0)];
        assert!(divide(&top, &tomorrow, Timeframe::days(1)).is_empty(), "never tomorrow's");
    }

    #[test]
    fn nothing_is_worked_out_from_a_price_too_old_or_by_dividing_by_zero() {
        let top = [flat(30 * DAY, 10.0)];
        assert!(divide(&top, &[flat(0, 5.0)], Timeframe::days(1)).is_empty());
        assert!(divide(&top, &[flat(29 * DAY, 0.0)], Timeframe::days(1)).is_empty());
        assert!(divide(&top, &[], Timeframe::days(1)).is_empty());
    }

    /// One over a bar: the high is one over the low, and the low one over
    /// the high, since the smaller the price the larger its reciprocal.
    #[test]
    fn a_pair_turned_over_has_its_range_swapped() {
        let one_over = Expr::Bin(Op::Div, Box::new(Expr::Num(1.0)), Box::new(Expr::Leg(0)));
        let out = combine(&one_over, &[vec![bar(0, 2.0, 4.0, 1.0, 2.5), flat(DAY, 0.0)]], Timeframe::days(1));
        assert_eq!(out.len(), 1, "a zero is not divided by");
        let r = out[0];
        assert_eq!((r.open, r.high, r.low, r.close), (0.5, 1.0, 0.25, 0.4));
    }

    struct Three;

    impl Provider for Three {
        fn id(&self) -> &'static str {
            "three"
        }
        fn label(&self) -> &'static str {
            "Three"
        }
        fn delay_minutes(&self, _kind: crate::symbols::InstrumentKind) -> u32 {
            0
        }
        fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
            Some(instrument.symbol.clone())
        }
        fn capabilities(&self) -> &'static [crate::provider::Capability] {
            &[]
        }
        fn bars(&self, symbol: &str, _: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
            let price = match symbol {
                "A" => 10.0,
                "B" => 4.0,
                "C" => 1.0,
                _ => return Err(ProviderError::NotFound),
            };
            Ok(vec![flat(since.unwrap_or(0), price)])
        }
    }

    #[test]
    fn a_formula_fetches_each_leg_and_a_plain_symbol_goes_through() {
        let since = Some(30 * DAY);
        let out = bars(&Three, "{A}/{B}", Timeframe::days(1), since).unwrap();
        assert_eq!(out.len(), 1, "the second leg was asked from before the first");
        assert_eq!(out[0].close, 2.5);
        let out = bars(&Three, "({A}-{B})*2+{C}", Timeframe::days(1), since).unwrap();
        assert_eq!(out[0].close, 13.0);
        assert_eq!(bars(&Three, "1/{B}", Timeframe::days(1), None).unwrap()[0].close, 0.25);
        assert_eq!(bars(&Three, "A", Timeframe::days(1), since).unwrap()[0].close, 10.0);
        assert!(matches!(bars(&Three, "{A}/{D}", Timeframe::days(1), since), Err(ProviderError::NotFound)));
        assert!(matches!(bars(&Three, "{A}/", Timeframe::days(1), since), Err(ProviderError::NotFound)));
    }
}
