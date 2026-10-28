# market-making-sim

Market making in a limit order book, in Rust: an Avellaneda–Stoikov market maker, a price-time priority matching engine, and (step by step) the pieces needed to run strategies against realistic order flow.

## Avellaneda–Stoikov market maker

The market maker quotes around a **reservation price** that is skewed against its inventory, with a spread set by risk aversion γ and order-flow parameters:

```
r(s, q, t)  = s − q γ σ² (T − t)
δ_a + δ_b   = γ σ² (T − t) + (2/γ) ln(1 + γ/k)
```

Fills arrive with intensity λ(δ) = A e^{−kδ}, where δ is the quote's distance from the mid.

### Reproducing the paper

`cargo run --release --bin reproduce_paper` runs the numerical experiment of Avellaneda and Stoikov (2008), Section 4 (s₀ = 100, T = 1, σ = 2, dt = 0.005, k = 1.5, A = 140, 1000 paths), comparing the inventory strategy with a symmetric benchmark that uses the same spread centred on the mid.

| γ | Strategy | Avg spread | Profit | Std(profit) | Final q | Std(final q) |
|---|---|---:|---:|---:|---:|---:|
| 0.1 | Inventory | 1.49 | 65.00 | 6.66 | 0.04 | 2.91 |
| 0.1 | Symmetric | 1.49 | 68.40 | 13.98 | −0.01 | 8.52 |
| 0.01 | Inventory | 1.35 | 68.40 | 8.98 | 0.10 | 5.23 |
| 0.01 | Symmetric | 1.35 | 69.10 | 14.48 | 0.01 | 8.89 |
| 0.5 | Inventory | 2.16 | 48.82 | 6.00 | −0.03 | 1.93 |
| 0.5 | Symmetric | 2.16 | 58.47 | 11.71 | −0.03 | 7.15 |

The main result of the paper holds: skewing quotes by inventory cuts both P&L and inventory dispersion for a small loss in mean profit. For γ = 0.1, the P&L standard deviation halves and the inventory standard deviation falls by a factor of about 3. The effect grows with γ.

The spread column here is the time average of the full spread, including the γσ²(T − t) term; the constant part (2/γ) ln(1 + γ/k) alone is 1.29 for γ = 0.1.

## Order book

`orderbook.rs` is a limit order book with price-time priority on integer price ticks: limit orders (marketable part trades, the rest rests), market orders, and cancels.

`cargo run --release --bin bench_orderbook` sends 2M random operations (45% limit, 45% cancel, 10% market) to a book that stays around 800 resting orders:

| Metric | Value |
|---|---:|
| Throughput | 3.3 M ops/s |
| Mean latency | ~300 ns/op |

Measured on a laptop (single thread, release build). Each price level is a `VecDeque`, so a cancel scans its level: when levels grow to thousands of orders, latency rises to ~11 µs/op. Fixing that is the first item on the roadmap.

## Layout

```
src/
  avellaneda_stoikov.rs   model, quoting strategies
  simulation.rs           Monte Carlo of the market maker
  orderbook.rs            matching engine
  bin/
    reproduce_paper.rs
    bench_orderbook.rs
```

```bash
cargo test
cargo run --release --bin reproduce_paper
cargo run --release --bin bench_orderbook
```

## Roadmap

- [x] Avellaneda–Stoikov strategy and reproduction of the paper's experiment
- [x] Price-time priority order book with tests and a throughput benchmark
- [ ] O(1) cancel: order slab + intrusive linked list per level, with before/after benchmark
- [ ] Latency percentiles (p50 / p99 / p99.9) instead of the mean
- [ ] Run the market maker against the order book with Poisson order flow, so queue position matters
- [ ] Record L2 order book data from a crypto exchange WebSocket and replay it
- [ ] Estimate A and k from recorded data
- [ ] Order flow imbalance as a short-horizon signal; adverse selection analysis
- [ ] Inventory limits (Guéant, Lehalle and Fernandez-Tapia, 2013)

## References

- M. Avellaneda and S. Stoikov (2008), *High-frequency trading in a limit order book*, Quantitative Finance 8(3).
- O. Guéant, C.-A. Lehalle and J. Fernandez-Tapia (2013), *Dealing with the inventory risk: a solution to the market making problem*, Mathematics and Financial Economics 7(4).
