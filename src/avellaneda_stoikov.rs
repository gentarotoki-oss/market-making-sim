//! Avellaneda & Stoikov (2008), "High-frequency trading in a limit order book".
//!
//! A market maker quotes a bid and an ask around a mid-price that follows
//! dS = σ dW. A quote at distance δ from the mid is hit with intensity
//! λ(δ) = A e^{-kδ}. With exponential utility (risk aversion γ) and
//! horizon T, the approximate optimal quotes are
//!
//!   reservation price  r(s, q, t) = s - q γ σ² (T - t)
//!   total spread       δ_a + δ_b  = γ σ² (T - t) + (2/γ) ln(1 + γ/k)
//!
//! and the quotes are r ± spread / 2.

/// Model and market parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Initial mid-price.
    pub s0: f64,
    /// Horizon T.
    pub horizon: f64,
    /// Mid-price volatility σ.
    pub sigma: f64,
    /// Time step.
    pub dt: f64,
    /// Risk aversion γ.
    pub gamma: f64,
    /// Decay of fill intensity with distance from the mid.
    pub k: f64,
    /// Fill intensity at zero distance.
    pub a: f64,
}

impl Params {
    /// Parameters used in the paper's numerical section.
    pub fn paper(gamma: f64) -> Self {
        Self {
            s0: 100.0,
            horizon: 1.0,
            sigma: 2.0,
            dt: 0.005,
            gamma,
            k: 1.5,
            a: 140.0,
        }
    }

    pub fn n_steps(&self) -> usize {
        (self.horizon / self.dt).round() as usize
    }

    /// Probability that a quote at `distance` from the mid is filled within one step.
    pub fn fill_probability(&self, distance: f64) -> f64 {
        (self.a * (-self.k * distance).exp() * self.dt).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quote {
    pub bid: f64,
    pub ask: f64,
}

impl Quote {
    pub fn spread(&self) -> f64 {
        self.ask - self.bid
    }
}

/// A quoting rule: given time, mid-price and inventory, return a bid and an ask.
pub trait Strategy {
    fn quote(&self, t: f64, mid: f64, inventory: i64) -> Quote;
}

pub fn reservation_price(p: &Params, t: f64, mid: f64, inventory: i64) -> f64 {
    mid - inventory as f64 * p.gamma * p.sigma.powi(2) * (p.horizon - t)
}

pub fn optimal_spread(p: &Params, t: f64) -> f64 {
    p.gamma * p.sigma.powi(2) * (p.horizon - t) + (2.0 / p.gamma) * (1.0 + p.gamma / p.k).ln()
}

/// The paper's strategy: quotes centred on the reservation price.
pub struct InventoryStrategy {
    pub params: Params,
}

impl Strategy for InventoryStrategy {
    fn quote(&self, t: f64, mid: f64, inventory: i64) -> Quote {
        let r = reservation_price(&self.params, t, mid, inventory);
        let half = optimal_spread(&self.params, t) / 2.0;
        Quote {
            bid: r - half,
            ask: r + half,
        }
    }
}

/// Benchmark from the paper: the same spread, centred on the mid-price,
/// so it ignores inventory.
pub struct SymmetricStrategy {
    pub params: Params,
}

impl Strategy for SymmetricStrategy {
    fn quote(&self, t: f64, mid: f64, _inventory: i64) -> Quote {
        let half = optimal_spread(&self.params, t) / 2.0;
        Quote {
            bid: mid - half,
            ask: mid + half,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_inventory_lowers_reservation_price() {
        let p = Params::paper(0.1);
        assert!(reservation_price(&p, 0.0, 100.0, 5) < 100.0);
        assert!(reservation_price(&p, 0.0, 100.0, -5) > 100.0);
        assert_eq!(reservation_price(&p, 0.0, 100.0, 0), 100.0);
    }

    #[test]
    fn inventory_skew_vanishes_at_horizon() {
        let p = Params::paper(0.1);
        assert_eq!(reservation_price(&p, p.horizon, 100.0, 10), 100.0);
    }

    #[test]
    fn spread_matches_closed_form() {
        // γ = 0.1, σ = 2, k = 1.5, t = 0: 0.1·4·1 + 20·ln(1 + 0.1/1.5)
        let p = Params::paper(0.1);
        let expected = 0.4 + 20.0 * (1.0f64 + 0.1 / 1.5).ln();
        assert!((optimal_spread(&p, 0.0) - expected).abs() < 1e-12);
    }

    #[test]
    fn symmetric_strategy_is_centred_on_mid() {
        let p = Params::paper(0.1);
        let q = SymmetricStrategy { params: p }.quote(0.3, 100.0, 7);
        assert!(((q.bid + q.ask) / 2.0 - 100.0).abs() < 1e-12);
    }

    #[test]
    fn fill_probability_decreases_with_distance() {
        let p = Params::paper(0.1);
        assert!(p.fill_probability(0.5) > p.fill_probability(1.0));
        assert!(p.fill_probability(-10.0) <= 1.0);
    }
}
