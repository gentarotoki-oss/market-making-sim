//! Reproduces the numerical comparison in Avellaneda & Stoikov (2008), Section 4:
//! the inventory strategy against a symmetric benchmark, 1000 paths each.
//!
//! cargo run --release --bin reproduce_paper

use market_making_sim::avellaneda_stoikov::{InventoryStrategy, Params, SymmetricStrategy};
use market_making_sim::simulation::{Summary, run};

const N_PATHS: usize = 1000;
const SEED: u64 = 42;

fn print_row(name: &str, s: &Summary) {
    println!(
        "{:<10} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
        name, s.mean_spread, s.mean_pnl, s.std_pnl, s.mean_inventory, s.std_inventory
    );
}

fn main() {
    for gamma in [0.1, 0.01, 0.5] {
        let p = Params::paper(gamma);
        println!("γ = {gamma}");
        println!(
            "{:<10} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "strategy", "spread", "profit", "std(P)", "final q", "std(q)"
        );
        print_row(
            "inventory",
            &run(&p, &InventoryStrategy { params: p }, N_PATHS, SEED),
        );
        print_row(
            "symmetric",
            &run(&p, &SymmetricStrategy { params: p }, N_PATHS, SEED),
        );
        println!();
    }
}
