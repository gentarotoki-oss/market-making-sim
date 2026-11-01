//! Records the Binance USD-M futures diff-depth stream for one symbol, plus a
//! REST snapshot, as JSON lines with the local receive time:
//!
//!   {"kind":"update","recv_ns":...,"data":{...stream event...}}
//!   {"kind":"snapshot","recv_ns":...,"data":{...REST response...}}
//!
//! The stream is opened first and the snapshot fetched while it buffers, so
//! the recording always contains the events needed to bridge the snapshot.
//!
//! cargo run --release --bin record_binance -- btcusdt 600 data/btcusdt.jsonl

use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tungstenite::Message;

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

    let (mut ws, _) =
        tungstenite::connect(format!("wss://fstream.binance.com/ws/{symbol}@depth@100ms"))?;

    let mut updates = 0usize;
    let mut snapshot_taken = false;
    let deadline = Instant::now() + Duration::from_secs(seconds);

    while Instant::now() < deadline {
        match ws.read()? {
            Message::Text(text) => {
                write_line(&mut out, "update", now_ns(), text.as_str())?;
                updates += 1;
            }
            Message::Close(_) => break,
            _ => {} // pings are answered by tungstenite
        }

        if !snapshot_taken {
            let url = format!(
                "https://fapi.binance.com/fapi/v1/depth?symbol={}&limit=1000",
                symbol.to_uppercase()
            );
            let body = ureq::get(&url).call()?.body_mut().read_to_string()?;
            write_line(&mut out, "snapshot", now_ns(), &body)?;
            snapshot_taken = true;
        }
    }

    out.flush()?;
    println!("recorded {updates} updates and 1 snapshot to {path}");
    Ok(())
}
