//! Records the Binance USD-M futures diff-depth and trade streams (one message per trade) for one
//! symbol, plus REST snapshots, as JSON lines with the local receive time:
//!
//!   {"kind":"connect","recv_ns":...,"data":null}
//!   {"kind":"update","recv_ns":...,"data":{...depth event...}}
//!   {"kind":"snapshot","recv_ns":...,"data":{...REST response...}}
//!   {"kind":"trade","recv_ns":...,"data":{...trade event...}}
//!
//! On each connection the stream is opened first and the snapshot fetched
//! while it buffers, so every segment contains the events needed to bridge
//! its snapshot. If the connection drops, the recorder reconnects and starts
//! a new segment with a fresh snapshot.
//!
//! cargo run --release --bin record_binance -- btcusdt 600 data/btcusdt.jsonl

use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::value::RawValue;
use tungstenite::Message;

/// A message on a combined stream: `{"stream": "...", "data": {...}}`.
#[derive(Deserialize)]
struct Envelope<'a> {
    stream: &'a str,
    #[serde(borrow)]
    data: &'a RawValue,
}

#[derive(Default)]
struct Counts {
    updates: usize,
    trades: usize,
    snapshots: usize,
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_nanos()
}

fn write_line(out: &mut impl Write, kind: &str, recv_ns: u128, data: &str) -> std::io::Result<()> {
    writeln!(
        out,
        r#"{{"kind":"{kind}","recv_ns":{recv_ns},"data":{}}}"#,
        data.trim()
    )
}

/// Records one connection until the deadline. Returns an error if the
/// connection fails, so the caller can reconnect.
fn record_segment(
    symbol: &str,
    deadline: Instant,
    out: &mut BufWriter<File>,
    counts: &mut Counts,
) -> Result<(), Box<dyn Error>> {
    let (mut ws, _) = tungstenite::connect(format!(
        "wss://fstream.binance.com/stream?streams={symbol}@depth@100ms/{symbol}@trade"
    ))?;
    write_line(out, "connect", now_ns(), "null")?;
    let mut snapshot_taken = false;

    while Instant::now() < deadline {
        match ws.read()? {
            Message::Text(text) => {
                let recv = now_ns();
                let msg: Envelope = serde_json::from_str(text.as_str())?;
                if msg.stream.ends_with("@trade") {
                    write_line(out, "trade", recv, msg.data.get())?;
                    counts.trades += 1;
                } else {
                    write_line(out, "update", recv, msg.data.get())?;
                    counts.updates += 1;
                }
            }
            Message::Close(_) => return Err("server closed the connection".into()),
            _ => {} // pings are answered by tungstenite
        }

        if !snapshot_taken {
            let url = format!(
                "https://fapi.binance.com/fapi/v1/depth?symbol={}&limit=1000",
                symbol.to_uppercase()
            );
            let body = ureq::get(&url).call()?.body_mut().read_to_string()?;
            write_line(out, "snapshot", now_ns(), &body)?;
            counts.snapshots += 1;
            snapshot_taken = true;
        }
        if (counts.updates + counts.trades).is_multiple_of(1000) {
            out.flush()?;
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let symbol = args.get(1).map_or("btcusdt", String::as_str).to_lowercase();
    let seconds: u64 = args.get(2).map_or(Ok(60), |s| s.parse())?;
    let path = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| format!("data/{symbol}.jsonl"));
    if let Some(dir) = Path::new(&path).parent() {
        fs::create_dir_all(dir)?;
    }
    let mut out = BufWriter::new(File::create(&path)?);

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut counts = Counts::default();
    while Instant::now() < deadline {
        match record_segment(&symbol, deadline, &mut out, &mut counts) {
            Ok(()) => break,
            Err(e) => {
                out.flush()?;
                eprintln!("connection lost ({e}); reconnecting");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    out.flush()?;
    println!(
        "recorded {} depth updates, {} trades and {} snapshots to {path}",
        counts.updates, counts.trades, counts.snapshots
    );
    Ok(())
}
