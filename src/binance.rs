//! Binance USD-M futures depth feed: message formats and local book sync.
//!
//! A diff-depth event says "these levels now have these sizes" for every
//! change between update IDs `U` and `u`. To build a book from it, the
//! documented procedure is:
//!
//! 1. buffer the stream and fetch a REST snapshot (with `lastUpdateId`);
//! 2. drop events with `u < lastUpdateId`;
//! 3. the first applied event must straddle the snapshot:
//!    `U <= lastUpdateId <= u`;
//! 4. after that, each event's `pu` must equal the previous event's `u`,
//!    otherwise an update was missed and the book has to be rebuilt.

use serde::Deserialize;

use crate::l2book::{L2Book, Lots};
use crate::orderbook::{Price, Side};

/// REST snapshot from `GET /fapi/v1/depth`.
#[derive(Debug, Deserialize)]
pub struct DepthSnapshot {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
}

/// One `<symbol>@depth` stream event.
#[derive(Debug, Deserialize)]
pub struct DepthUpdate {
    /// Event time (exchange clock, ms).
    #[serde(rename = "E")]
    pub event_time: u64,
    /// First update ID in the event.
    #[serde(rename = "U")]
    pub first_id: u64,
    /// Final update ID in the event.
    #[serde(rename = "u")]
    pub final_id: u64,
    /// Final update ID of the previous event.
    #[serde(rename = "pu")]
    pub prev_final_id: u64,
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

/// How a symbol's decimal prices and sizes map to integer ticks and lots.
#[derive(Debug, Clone, Copy)]
pub struct Scale {
    pub price_decimals: u32,
    pub size_decimals: u32,
}

/// BTCUSDT: tick 0.10, lot 0.001.
pub const BTCUSDT: Scale = Scale {
    price_decimals: 1,
    size_decimals: 3,
};

/// Parses a decimal string into an integer number of `10^-decimals` units,
/// exactly. Trailing zeros beyond `decimals` are accepted ("77750.00" with one
/// decimal is 777500); any other extra digit is an error.
pub fn parse_fixed(s: &str, decimals: u32) -> Option<i64> {
    let (int_part, frac_part) = s.split_once('.').unwrap_or((s, ""));
    let (kept, extra) = frac_part.split_at(frac_part.len().min(decimals as usize));
    if !extra.bytes().all(|b| b == b'0') {
        return None;
    }
    let mut value: i64 = int_part.parse().ok()?;
    for d in kept.bytes() {
        if !d.is_ascii_digit() {
            return None;
        }
        value = value * 10 + (d - b'0') as i64;
    }
    for _ in kept.len()..decimals as usize {
        value *= 10;
    }
    Some(value)
}

#[derive(Debug, PartialEq)]
pub enum SyncError {
    /// The first event after the snapshot does not straddle it.
    NoOverlap {
        snapshot_id: u64,
        first_id: u64,
        final_id: u64,
    },
    /// `pu` did not match the previous `u`: an event was missed.
    Gap {
        expected: u64,
        got: u64,
    },
    BadNumber(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::NoOverlap {
                snapshot_id,
                first_id,
                final_id,
            } => write!(
                f,
                "first event [{first_id}, {final_id}] does not cover snapshot {snapshot_id}"
            ),
            SyncError::Gap { expected, got } => {
                write!(f, "missed an event: expected pu {expected}, got {got}")
            }
            SyncError::BadNumber(s) => write!(f, "not a decimal number: {s:?}"),
        }
    }
}

impl std::error::Error for SyncError {}

#[derive(Debug, PartialEq)]
pub enum Applied {
    /// Older than the snapshot; ignored.
    Stale,
    /// Applied, changing this many levels.
    Levels(usize),
}

/// A local book kept in sync with the diff-depth stream.
pub struct BookSync {
    pub book: L2Book,
    scale: Scale,
    snapshot_id: u64,
    last_final_id: Option<u64>,
}

impl BookSync {
    pub fn from_snapshot(snapshot: &DepthSnapshot, scale: Scale) -> Result<Self, SyncError> {
        let mut sync = Self {
            book: L2Book::new(),
            scale,
            snapshot_id: snapshot.last_update_id,
            last_final_id: None,
        };
        sync.apply_levels(Side::Buy, &snapshot.bids)?;
        sync.apply_levels(Side::Sell, &snapshot.asks)?;
        Ok(sync)
    }

    pub fn apply(&mut self, update: &DepthUpdate) -> Result<Applied, SyncError> {
        match self.last_final_id {
            None => {
                if update.final_id < self.snapshot_id {
                    return Ok(Applied::Stale);
                }
                if update.first_id > self.snapshot_id {
                    return Err(SyncError::NoOverlap {
                        snapshot_id: self.snapshot_id,
                        first_id: update.first_id,
                        final_id: update.final_id,
                    });
                }
            }
            Some(last) if update.prev_final_id != last => {
                return Err(SyncError::Gap {
                    expected: last,
                    got: update.prev_final_id,
                });
            }
            Some(_) => {}
        }
        let n = self.apply_levels(Side::Buy, &update.bids)?
            + self.apply_levels(Side::Sell, &update.asks)?;
        self.last_final_id = Some(update.final_id);
        Ok(Applied::Levels(n))
    }

    fn apply_levels(&mut self, side: Side, levels: &[[String; 2]]) -> Result<usize, SyncError> {
        for [price, size] in levels {
            let p: Price = parse_fixed(price, self.scale.price_decimals)
                .ok_or_else(|| SyncError::BadNumber(price.clone()))?;
            let q = parse_fixed(size, self.scale.size_decimals)
                .ok_or_else(|| SyncError::BadNumber(size.clone()))?;
            self.book.set(side, p, q as Lots);
        }
        Ok(levels.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(p: &str, q: &str) -> [String; 2] {
        [p.to_string(), q.to_string()]
    }

    fn update(first: u64, last: u64, prev: u64, bids: Vec<[String; 2]>) -> DepthUpdate {
        DepthUpdate {
            event_time: 0,
            first_id: first,
            final_id: last,
            prev_final_id: prev,
            bids,
            asks: vec![],
        }
    }

    fn snapshot() -> DepthSnapshot {
        DepthSnapshot {
            last_update_id: 100,
            bids: vec![lv("77750.00", "1.000")],
            asks: vec![lv("77750.10", "2.500")],
        }
    }

    #[test]
    fn parses_decimals_exactly() {
        assert_eq!(parse_fixed("77750.00", 1), Some(777_500));
        assert_eq!(parse_fixed("77750.1", 1), Some(777_501));
        assert_eq!(parse_fixed("77750", 1), Some(777_500));
        assert_eq!(parse_fixed("0.048", 3), Some(48));
        assert_eq!(parse_fixed("14.2", 3), Some(14_200));
        assert_eq!(parse_fixed("77750.05", 1), None);
        assert_eq!(parse_fixed("abc", 1), None);
    }

    #[test]
    fn snapshot_builds_the_book() {
        let sync = BookSync::from_snapshot(&snapshot(), BTCUSDT).unwrap();
        assert_eq!(sync.book.best_bid(), Some((777_500, 1_000)));
        assert_eq!(sync.book.best_ask(), Some((777_501, 2_500)));
    }

    #[test]
    fn drops_stale_events_then_requires_overlap() {
        let mut sync = BookSync::from_snapshot(&snapshot(), BTCUSDT).unwrap();
        assert_eq!(sync.apply(&update(90, 99, 89, vec![])), Ok(Applied::Stale));
        assert_eq!(
            sync.apply(&update(95, 105, 94, vec![lv("77749.90", "0.5")])),
            Ok(Applied::Levels(1))
        );
        assert_eq!(sync.book.levels(Side::Buy), 2);
    }

    #[test]
    fn first_event_after_snapshot_must_straddle_it() {
        let mut sync = BookSync::from_snapshot(&snapshot(), BTCUSDT).unwrap();
        assert!(matches!(
            sync.apply(&update(101, 110, 100, vec![])),
            Err(SyncError::NoOverlap { .. })
        ));
    }

    #[test]
    fn detects_a_missed_event() {
        let mut sync = BookSync::from_snapshot(&snapshot(), BTCUSDT).unwrap();
        sync.apply(&update(95, 105, 94, vec![])).unwrap();
        sync.apply(&update(106, 110, 105, vec![])).unwrap();
        assert_eq!(
            sync.apply(&update(115, 120, 112, vec![])),
            Err(SyncError::Gap {
                expected: 110,
                got: 112
            })
        );
    }

    #[test]
    fn zero_size_removes_a_level() {
        let mut sync = BookSync::from_snapshot(&snapshot(), BTCUSDT).unwrap();
        sync.apply(&update(95, 105, 94, vec![lv("77750.00", "0.000")]))
            .unwrap();
        assert_eq!(sync.book.best_bid(), None);
    }

    #[test]
    fn parses_stream_and_snapshot_json() {
        let u: DepthUpdate = serde_json::from_str(
            r#"{"e":"depthUpdate","E":1793502828772,"T":1793502828769,"s":"BTCUSDT",
               "U":10,"u":12,"pu":9,"b":[["77750.00","14.216"]],"a":[]}"#,
        )
        .unwrap();
        assert_eq!((u.first_id, u.final_id, u.prev_final_id), (10, 12, 9));
        let s: DepthSnapshot = serde_json::from_str(
            r#"{"lastUpdateId":11,"E":1,"T":1,"bids":[["1.0","2"]],"asks":[]}"#,
        )
        .unwrap();
        assert_eq!(s.last_update_id, 11);
    }
}
