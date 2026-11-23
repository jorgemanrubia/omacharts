//! Studies of what traded at each price and on which side.
//!
//! A bar's open, high, low and close say where price went; these read who
//! took it there. They need a bar that knows its volume at each price and
//! how much of it was bought at the offer and sold at the bid — an
//! [`AtPrice`] — which only data with the aggressor of every print can
//! give. A bar estimated from its range is not one, and these never
//! estimate.
//!
//! Prices here are whole ticks (`i32`) of a tick size the study is made
//! with, the way volume at price is counted, so sums are exact.

use std::cell::RefCell;

use crate::Ohlcv;

/// A bar that knows what traded at each price, and on which side.
pub trait AtPrice: Ohlcv {
    /// Volume that lifted the offer: buyers taking liquidity.
    fn bought(&self) -> u64;

    /// Volume that hit the bid: sellers taking liquidity.
    fn sold(&self) -> u64;

    /// Every price that traded, as `(tick, volume)`, lowest first. Volume
    /// whose side nobody reported is in it; it is on neither side above.
    fn each_price(&self, each: impl FnMut(i32, u64));

    /// Bought less sold, piece by piece in the order it traded — a minute
    /// at a time, say — so a running delta read from the bar shows how far
    /// it went either way inside it. One piece, the whole bar's, unless the
    /// bar knows its pieces.
    #[inline]
    fn each_delta(&self, mut each: impl FnMut(i64)) {
        each(self.bought() as i64 - self.sold() as i64);
    }
}

/// An indicator of volume at price, one bar at a time: [`crate::Study`]
/// for bars that are [`AtPrice`].
pub trait AtPriceStudy {
    type Value: Copy;

    /// Take the next bar, which has closed. `new_period` says it opens a
    /// period the study restarts at.
    fn step<B: AtPrice>(&mut self, bar: &B, new_period: bool) -> Option<Self::Value>;

    /// What [`AtPriceStudy::step`] would answer for `bar`, without taking
    /// it, to the bit.
    fn peek<B: AtPrice>(&self, bar: &B, new_period: bool) -> Option<Self::Value>;

    /// Bars taken before the first value can appear.
    fn warmup(&self) -> usize;

    /// Its key, as [`crate::catalog::AT_PRICE`] names it.
    fn key(&self) -> &'static str;

    /// Its parameters, by name.
    fn params(&self) -> Vec<(&'static str, f64)>;
}

/// [`VolumeDelta`]'s reading at a bar.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DeltaReading {
    /// Bought less sold, summed over the period so far, this bar included:
    /// the cumulative delta.
    pub cvd: f64,
    /// Bought less sold in this bar.
    pub delta: f64,
    pub bought: f64,
    pub sold: f64,
}

/// Volume delta, from the aggressor of every print: the bar's own, and
/// summed over the period — the cumulative delta, read rather than
/// estimated.
#[derive(Clone, Debug, Default)]
pub struct VolumeDelta {
    /// Kept whole, so a period of any length sums exactly.
    cvd: i64,
}

impl VolumeDelta {
    pub fn new() -> VolumeDelta {
        VolumeDelta::default()
    }

    #[inline]
    fn reading<B: AtPrice>(&self, bar: &B, new_period: bool) -> (i64, DeltaReading) {
        let (bought, sold) = (bar.bought(), bar.sold());
        let delta = bought as i64 - sold as i64;
        let cvd = if new_period { 0 } else { self.cvd } + delta;
        (cvd, DeltaReading { cvd: cvd as f64, delta: delta as f64, bought: bought as f64, sold: sold as f64 })
    }
}

impl AtPriceStudy for VolumeDelta {
    type Value = DeltaReading;

    #[inline]
    fn step<B: AtPrice>(&mut self, bar: &B, new_period: bool) -> Option<DeltaReading> {
        let (cvd, reading) = self.reading(bar, new_period);
        self.cvd = cvd;
        Some(reading)
    }

    #[inline]
    fn peek<B: AtPrice>(&self, bar: &B, new_period: bool) -> Option<DeltaReading> {
        Some(self.reading(bar, new_period).1)
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "volume_delta"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        Vec::new()
    }
}

/// [`DevelopingProfile`]'s reading at a bar: prices of whole ticks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ProfileReading {
    /// The price that traded most so far in the period; the higher of two
    /// that tie.
    pub poc: f64,
    /// The highest price of the value area.
    pub vah: f64,
    /// The lowest price of the value area.
    pub val: f64,
}

/// The volume profile of the period so far, one tick a row: where it has
/// traded most, and the value area around it, as known at each bar.
///
/// A whole period's profile is only whole once the period is over; this is
/// what could be known at every bar along the way.
/// The point of control and the value area are found the way a chart's
/// profile finds them: the busiest row, the higher on a tie, and then
/// outward from it taking the busier neighbour — the upper on a tie —
/// until the rows hold the value-area fraction of the volume.
#[derive(Clone, Debug)]
pub struct DevelopingProfile {
    tick_size: f64,
    value_area: f64,
    /// Volume at each tick from `low`, for the period so far.
    volumes: Vec<u64>,
    low: i32,
    total: u64,
    /// Room to work out a bar not yet taken, for [`AtPriceStudy::peek`].
    scratch: RefCell<Vec<u64>>,
}

/// Ticks of room a profile starts with: a busy day of the busiest contracts
/// fits, so a period rarely has to grow.
const ROOM: usize = 2048;

impl DevelopingProfile {
    /// A profile in ticks of `tick_size`, with a value area of 70%.
    pub fn new(tick_size: f64) -> DevelopingProfile {
        DevelopingProfile {
            tick_size,
            value_area: 0.7,
            volumes: Vec::with_capacity(ROOM),
            low: 0,
            total: 0,
            scratch: RefCell::new(Vec::with_capacity(ROOM)),
        }
    }

    /// The share of the volume the value area holds, `0.0..=1.0`.
    pub fn value_area(mut self, fraction: f64) -> DevelopingProfile {
        self.value_area = fraction;
        self
    }

    /// Add `bar`'s volume at each price into `volumes`, which starts at
    /// tick `low`, growing it either way as needed.
    #[inline]
    fn add<B: AtPrice>(volumes: &mut Vec<u64>, low: &mut i32, total: &mut u64, bar: &B) {
        bar.each_price(|tick, volume| {
            if volume == 0 {
                return;
            }
            if volumes.is_empty() {
                *low = tick;
            }
            // In i64: two ticks far apart are further apart than an i32.
            if tick < *low {
                let more = (i64::from(*low) - i64::from(tick)) as usize;
                volumes.splice(0..0, std::iter::repeat_n(0, more));
                *low = tick;
            }
            let at = (i64::from(tick) - i64::from(*low)) as usize;
            if at >= volumes.len() {
                volumes.resize(at + 1, 0);
            }
            volumes[at] += volume;
            *total += volume;
        });
    }

    /// The reading of a profile of `volumes` from tick `low`.
    fn read(&self, volumes: &[u64], low: i32, total: u64) -> Option<ProfileReading> {
        if total == 0 {
            return None;
        }
        // The chart's rule, on volumes as f64: the busiest row, the last of
        // equals, and the value area grown out from it.
        let poc = volumes.iter().enumerate().fold(0, |poc, (row, volume)| if *volume >= volumes[poc] { row } else { poc });
        let (lower, upper) =
            crate::volume_profile::value_area(volumes.len(), |row| volumes[row] as f64, poc, total as f64, self.value_area);
        let price = |row: usize| (i64::from(low) + row as i64) as f64 * self.tick_size;
        Some(ProfileReading { poc: price(poc), vah: price(upper), val: price(lower) })
    }
}

impl AtPriceStudy for DevelopingProfile {
    type Value = ProfileReading;

    fn step<B: AtPrice>(&mut self, bar: &B, new_period: bool) -> Option<ProfileReading> {
        if new_period {
            self.volumes.clear();
            self.total = 0;
        }
        Self::add(&mut self.volumes, &mut self.low, &mut self.total, bar);
        self.read(&self.volumes, self.low, self.total)
    }

    fn peek<B: AtPrice>(&self, bar: &B, new_period: bool) -> Option<ProfileReading> {
        let mut scratch = self.scratch.borrow_mut();
        scratch.clear();
        let (mut low, mut total) = (self.low, 0);
        if !new_period {
            scratch.extend_from_slice(&self.volumes);
            total = self.total;
        }
        Self::add(&mut scratch, &mut low, &mut total, bar);
        self.read(&scratch, low, total)
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "developing_profile"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("tick_size", self.tick_size), ("value_area", self.value_area)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ohlcv;

    /// A bar of `(tick, bought, sold)` prints.
    struct Prints(Vec<(i32, u64, u64)>);

    impl Ohlcv for Prints {
        fn ts(&self) -> i64 {
            0
        }
        fn open(&self) -> f64 {
            0.0
        }
        fn high(&self) -> f64 {
            0.0
        }
        fn low(&self) -> f64 {
            0.0
        }
        fn close(&self) -> f64 {
            0.0
        }
        fn volume(&self) -> f64 {
            self.0.iter().map(|(_, b, s)| (b + s) as f64).sum()
        }
    }

    impl AtPrice for Prints {
        fn bought(&self) -> u64 {
            self.0.iter().map(|p| p.1).sum()
        }
        fn sold(&self) -> u64 {
            self.0.iter().map(|p| p.2).sum()
        }
        fn each_price(&self, mut each: impl FnMut(i32, u64)) {
            let mut sorted = self.0.clone();
            sorted.sort_by_key(|p| p.0);
            for (tick, bought, sold) in sorted {
                each(tick, bought + sold);
            }
        }
    }

    fn bars(seed: u64, n: usize) -> Vec<Prints> {
        let mut state = seed;
        let mut next = move |below: u64| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) % below
        };
        let mut centre = 20_000i32;
        (0..n)
            .map(|_| {
                centre += next(9) as i32 - 4;
                Prints((0..1 + next(6)).map(|_| (centre + next(7) as i32 - 3, next(40), next(40))).collect())
            })
            .collect()
    }

    /// The profile of everything at once, worked out from scratch.
    fn brute(bars: &[Prints], tick: f64, fraction: f64) -> Option<ProfileReading> {
        let mut at = std::collections::BTreeMap::<i32, u64>::new();
        for bar in bars {
            bar.each_price(|t, v| *at.entry(t).or_default() += v);
        }
        let (low, high) = (*at.keys().next()?, *at.keys().last()?);
        let volumes: Vec<u64> = (low..=high).map(|t| at.get(&t).copied().unwrap_or(0)).collect();
        let total = volumes.iter().sum();
        DevelopingProfile::new(tick).value_area(fraction).read(&volumes, low, total)
    }

    #[test]
    fn the_cumulative_delta_is_the_sum_of_the_bars_and_starts_again_each_period() {
        let bars = bars(1, 300);
        let mut study = VolumeDelta::new();
        let mut cvd = 0i64;
        for (i, bar) in bars.iter().enumerate() {
            let new_period = i % 100 == 0;
            if new_period {
                cvd = 0;
            }
            let delta = bar.bought() as i64 - bar.sold() as i64;
            cvd += delta;
            let peeked = study.peek(bar, new_period);
            let reading = study.step(bar, new_period).unwrap();
            assert_eq!(peeked, Some(reading));
            assert_eq!((reading.cvd, reading.delta), (cvd as f64, delta as f64));
        }
    }

    #[test]
    fn a_developing_profile_is_the_profile_of_its_period_so_far() {
        let bars = bars(7, 400);
        let mut study = DevelopingProfile::new(0.25);
        let mut start = 0;
        for i in 0..bars.len() {
            let new_period = i % 130 == 0;
            if new_period {
                start = i;
            }
            let peeked = study.peek(&bars[i], new_period);
            let stepped = study.step(&bars[i], new_period);
            assert_eq!(peeked, stepped, "peek and step agree at bar {i}");
            assert_eq!(stepped, brute(&bars[start..=i], 0.25, 0.7), "bar {i}");
        }
    }

    #[test]
    fn the_point_of_control_and_the_value_area_break_ties_the_way_a_chart_does() {
        // Two rows tie for most: the higher is the point of control. Its
        // neighbours tie: the value area takes the upper first.
        let bar = Prints(vec![(10, 5, 0), (11, 2, 0), (12, 5, 0), (13, 2, 0)]);
        let reading = DevelopingProfile::new(1.0).value_area(0.5).step(&bar, true).unwrap();
        assert_eq!((reading.poc, reading.val, reading.vah), (12.0, 12.0, 13.0));
    }

    #[test]
    fn a_profile_with_nothing_traded_has_no_reading() {
        assert_eq!(DevelopingProfile::new(0.25).step(&Prints(vec![]), true), None);
    }
}
