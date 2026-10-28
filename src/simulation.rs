//! Monte Carlo simulation of a market maker in the Avellaneda–Stoikov setting.
//!
//! Each step:
//! 1. the strategy posts a bid and an ask,
//! 2. each side is filled independently with probability A e^{-kδ} dt,
//! 3. the mid-price moves up or down by σ√dt with equal probability.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::avellaneda_stoikov::{Params, Strategy};

#[derive(Debug, Clone, Copy)]
pub struct PathResult {
    /// Terminal P&L: cash plus inventory marked at the final mid.
    pub pnl: f64,
    pub final_inventory: i64,
    pub mean_spread: f64,
}

pub fn simulate_path<S: Strategy, R: Rng>(
    params: &Params,
    strategy: &S,
    rng: &mut R,
) -> PathResult {
    let n = params.n_steps();
    let step = params.sigma * params.dt.sqrt();

    let mut mid = params.s0;
    let mut cash = 0.0;
    let mut inventory: i64 = 0;
    let mut spread_sum = 0.0;

    for i in 0..n {
        let t = i as f64 * params.dt;
        let quote = strategy.quote(t, mid, inventory);
        spread_sum += quote.spread();

        if rng.r#gen::<f64>() < params.fill_probability(mid - quote.bid) {
            inventory += 1;
            cash -= quote.bid;
        }
        if rng.r#gen::<f64>() < params.fill_probability(quote.ask - mid) {
            inventory -= 1;
            cash += quote.ask;
        }

        mid += if rng.gen_bool(0.5) { step } else { -step };
    }

    PathResult {
        pnl: cash + inventory as f64 * mid,
        final_inventory: inventory,
        mean_spread: spread_sum / n as f64,
    }
}

/// Summary over many simulated paths.
#[derive(Debug, Clone, Copy)]
pub struct Summary {
    pub mean_spread: f64,
    pub mean_pnl: f64,
    pub std_pnl: f64,
    pub mean_inventory: f64,
    pub std_inventory: f64,
}

pub fn run<S: Strategy>(params: &Params, strategy: &S, n_paths: usize, seed: u64) -> Summary {
    let mut rng = StdRng::seed_from_u64(seed);
    let results: Vec<PathResult> = (0..n_paths)
        .map(|_| simulate_path(params, strategy, &mut rng))
        .collect();

    let spreads: Vec<f64> = results.iter().map(|r| r.mean_spread).collect();
    let pnls: Vec<f64> = results.iter().map(|r| r.pnl).collect();
    let inventories: Vec<f64> = results.iter().map(|r| r.final_inventory as f64).collect();

    Summary {
        mean_spread: mean(&spreads),
        mean_pnl: mean(&pnls),
        std_pnl: std_dev(&pnls),
        mean_inventory: mean(&inventories),
        std_inventory: std_dev(&inventories),
    }
}

fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample standard deviation.
fn std_dev(xs: &[f64]) -> f64 {
    let m = mean(xs);
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    var.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avellaneda_stoikov::{InventoryStrategy, SymmetricStrategy};

    #[test]
    fn same_seed_gives_same_result() {
        let p = Params::paper(0.1);
        let s = InventoryStrategy { params: p };
        let a = run(&p, &s, 50, 7);
        let b = run(&p, &s, 50, 7);
        assert_eq!(a.mean_pnl, b.mean_pnl);
    }

    #[test]
    fn inventory_strategy_has_lower_risk_than_symmetric() {
        // The paper's main result: skewing quotes by inventory cuts P&L and
        // inventory dispersion at a small cost in mean P&L.
        let p = Params::paper(0.1);
        let inv = run(&p, &InventoryStrategy { params: p }, 1000, 1);
        let sym = run(&p, &SymmetricStrategy { params: p }, 1000, 1);

        assert!(inv.std_pnl < sym.std_pnl);
        assert!(inv.std_inventory < sym.std_inventory);
        assert!(inv.mean_pnl < sym.mean_pnl);
    }
}
