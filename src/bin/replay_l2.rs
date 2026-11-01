//! Replays a recording from `record_binance`: rebuilds the book from the
//! snapshot and every update, checks sequencing and that the book never
//! crosses, and reports the time to parse and apply each update.
//!
//! cargo run --release --bin replay_l2 -- data/btcusdt.jsonl

use std::env;
use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::time::Instant;

use serde::Deserialize;
use serde_json::value::RawValue;

use market_making_sim::binance::{Applied, BTCUSDT, BookSync, DepthSnapshot, DepthUpdate};
use market_making_sim::orderbook::Side;

#[derive(Deserialize)]
struct Line<'a> {
    kind: &'a str,
    recv_ns: u64,
    #[serde(borrow)]
    data: &'a RawValue,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    sorted[((p / 100.0) * (sorted.len() - 1) as f64).round() as usize]
}

fn report(name: &str, mut xs: Vec<u64>, unit: &str) {
    xs.sort_unstable();
    let mean = xs.iter().sum::<u64>() as f64 / xs.len() as f64;
    println!(
        "  {name:<22} mean {mean:>8.0}  p50 {:>7}  p99 {:>7}  p99.9 {:>7}  max {:>7}  ({unit})",
        percentile(&xs, 50.0),
        percentile(&xs, 99.0),
        percentile(&xs, 99.9),
        xs[xs.len() - 1],
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: replay_l2 <recording.jsonl>")?;
    let text = fs::read_to_string(&path)?;
    let lines: Vec<Line> = text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;

    let snapshot_line = lines
        .iter()
        .find(|l| l.kind == "snapshot")
        .ok_or("no snapshot in recording")?;
    let snapshot: DepthSnapshot = serde_json::from_str(snapshot_line.data.get())?;
    let mut sync = BookSync::from_snapshot(&snapshot, BTCUSDT)?;

    let mut parse_ns = Vec::new();
    let mut apply_ns = Vec::new();
    let mut levels_per_update = Vec::new();
    let mut delay_ms = Vec::new(); // receive time minus exchange event time
    let (mut stale, mut crossed) = (0usize, 0usize);

    for line in lines.iter().filter(|l| l.kind == "update") {
        let t0 = Instant::now();
        let update: DepthUpdate = serde_json::from_str(line.data.get())?;
        let t1 = Instant::now();
        let applied = sync.apply(black_box(&update));
        let t2 = Instant::now();

        match applied {
            Ok(Applied::Stale) => stale += 1,
            Ok(Applied::Levels(n)) => {
                parse_ns.push((t1 - t0).as_nanos() as u64);
                apply_ns.push((t2 - t1).as_nanos() as u64);
                levels_per_update.push(n as u64);
                delay_ms.push(line.recv_ns as i64 / 1_000_000 - update.event_time as i64);
                if sync.book.is_crossed() {
                    crossed += 1;
                }
            }
            Err(e) => {
                return Err(format!("sync failed at update {}: {e:?}", update.final_id).into());
            }
        }
    }

    let n = apply_ns.len();
    let secs = (lines.last().unwrap().recv_ns - lines[0].recv_ns) as f64 / 1e9;
    println!("{path}");
    println!("  {n} updates applied over {secs:.0} s ({stale} older than the snapshot, dropped)");
    println!("  sequence: no gaps; book crossed after {crossed} updates");
    report("levels per update", levels_per_update, "levels");
    report("parse JSON", parse_ns, "ns");
    report("apply to book", apply_ns, "ns");

    // The local clock is not synchronized with the exchange's, so only the
    // spread of the delay is meaningful, not its level.
    let min = *delay_ms.iter().min().unwrap();
    report(
        "delay jitter",
        delay_ms.iter().map(|d| (d - min) as u64).collect(),
        "ms above minimum",
    );

    let (bid, bid_size) = sync.book.best_bid().unwrap();
    let (ask, ask_size) = sync.book.best_ask().unwrap();
    println!(
        "  final book: {} bid / {} ask levels, best {:.1} x {:.3} | {:.1} x {:.3}",
        sync.book.levels(Side::Buy),
        sync.book.levels(Side::Sell),
        bid as f64 / 10.0,
        bid_size as f64 / 1000.0,
        ask as f64 / 10.0,
        ask_size as f64 / 1000.0,
    );
    Ok(())
}
