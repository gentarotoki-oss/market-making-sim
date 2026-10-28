//! Throughput of the order book under random order flow:
//! 45% limit orders around the mid, 45% cancels, 10% market orders, so the
//! book stays at a roughly constant size.
//!
//! cargo run --release --bin bench_orderbook

use std::hint::black_box;
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use market_making_sim::orderbook::{OrderBook, OrderId, Side};

const N_OPS: usize = 2_000_000;

fn main() {
    let mut rng = StdRng::seed_from_u64(0);
    let mut book = OrderBook::new();
    let mut live: Vec<OrderId> = Vec::new();
    let mut next_id: OrderId = 0;
    let mut fills = 0usize;

    let start = Instant::now();
    for _ in 0..N_OPS {
        let side = if rng.gen_bool(0.5) { Side::Buy } else { Side::Sell };
        let r: f64 = rng.r#gen();
        if r < 0.45 {
            let offset = rng.gen_range(1..=20);
            let price = match side {
                Side::Buy => 10_000 - offset + 3,
                Side::Sell => 10_000 + offset - 3,
            };
            fills += book.limit(next_id, side, price, rng.gen_range(1..=100)).len();
            live.push(next_id);
            next_id += 1;
        } else if r < 0.9 && !live.is_empty() {
            let i = rng.gen_range(0..live.len());
            black_box(book.cancel(live.swap_remove(i)));
        } else {
            fills += book.market(next_id, side, rng.gen_range(1..=100)).len();
            next_id += 1;
        }
    }
    let elapsed = start.elapsed();

    println!("operations:   {N_OPS}");
    println!("fills:        {fills}");
    println!("resting:      {}", book.order_count());
    println!("elapsed:      {:.3} s", elapsed.as_secs_f64());
    println!("throughput:   {:.2} M ops/s", N_OPS as f64 / elapsed.as_secs_f64() / 1e6);
    println!("mean latency: {:.0} ns/op", elapsed.as_nanos() as f64 / N_OPS as f64);
}
