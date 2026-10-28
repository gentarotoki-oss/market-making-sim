//! A limit order book with price-time priority.
//!
//! Prices are integer ticks so that levels can be keyed exactly. Each price
//! level is a FIFO queue; an incoming order trades against the best opposite
//! level first and, within a level, against the oldest resting order first.

use std::collections::{BTreeMap, HashMap, VecDeque};

pub type OrderId = u64;
pub type Price = i64;
pub type Qty = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub maker: OrderId,
    pub taker: OrderId,
    pub price: Price,
    pub qty: Qty,
}

#[derive(Debug, Clone, Copy)]
struct Resting {
    id: OrderId,
    qty: Qty,
}

#[derive(Debug, Default)]
pub struct OrderBook {
    bids: BTreeMap<Price, VecDeque<Resting>>,
    asks: BTreeMap<Price, VecDeque<Resting>>,
    /// Where each resting order sits, for cancels.
    index: HashMap<OrderId, (Side, Price)>,
}

impl OrderBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn best_bid(&self) -> Option<Price> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<Price> {
        self.asks.keys().next().copied()
    }

    /// Total resting quantity at a price level.
    pub fn depth(&self, side: Side, price: Price) -> Qty {
        let book = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        book.get(&price).map_or(0, |level| level.iter().map(|o| o.qty).sum())
    }

    pub fn order_count(&self) -> usize {
        self.index.len()
    }

    /// Submits a limit order. The marketable part trades immediately and the
    /// remainder rests in the book.
    pub fn limit(&mut self, id: OrderId, side: Side, price: Price, qty: Qty) -> Vec<Fill> {
        let mut fills = Vec::new();
        let remaining = self.match_against(id, side, qty, Some(price), &mut fills);
        if remaining > 0 {
            let book = match side {
                Side::Buy => &mut self.bids,
                Side::Sell => &mut self.asks,
            };
            book.entry(price).or_default().push_back(Resting { id, qty: remaining });
            self.index.insert(id, (side, price));
        }
        fills
    }

    /// Submits a market order. Any quantity the book cannot fill is dropped.
    pub fn market(&mut self, id: OrderId, side: Side, qty: Qty) -> Vec<Fill> {
        let mut fills = Vec::new();
        self.match_against(id, side, qty, None, &mut fills);
        fills
    }

    /// Cancels a resting order. Returns false if it is not in the book.
    pub fn cancel(&mut self, id: OrderId) -> bool {
        let Some((side, price)) = self.index.remove(&id) else {
            return false;
        };
        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        let level = book.get_mut(&price).expect("indexed order has a level");
        level.retain(|o| o.id != id);
        if level.is_empty() {
            book.remove(&price);
        }
        true
    }

    /// Trades `qty` against the opposite side, best price first, while the
    /// price is within `limit`. Returns the unfilled quantity.
    fn match_against(
        &mut self,
        taker: OrderId,
        side: Side,
        mut qty: Qty,
        limit: Option<Price>,
        fills: &mut Vec<Fill>,
    ) -> Qty {
        while qty > 0 {
            let best = match side {
                Side::Buy => self.asks.first_entry(),
                Side::Sell => self.bids.last_entry(),
            };
            let Some(mut level) = best else { break };
            let price = *level.key();
            let crosses = match (side, limit) {
                (_, None) => true,
                (Side::Buy, Some(l)) => price <= l,
                (Side::Sell, Some(l)) => price >= l,
            };
            if !crosses {
                break;
            }

            let queue = level.get_mut();
            while qty > 0 {
                let Some(maker) = queue.front_mut() else { break };
                let traded = qty.min(maker.qty);
                fills.push(Fill {
                    maker: maker.id,
                    taker,
                    price,
                    qty: traded,
                });
                qty -= traded;
                maker.qty -= traded;
                if maker.qty == 0 {
                    self.index.remove(&maker.id);
                    queue.pop_front();
                }
            }
            if queue.is_empty() {
                level.remove();
            }
        }
        qty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resting_orders_set_best_prices() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Buy, 99, 10);
        book.limit(2, Side::Buy, 98, 10);
        book.limit(3, Side::Sell, 101, 10);
        assert_eq!(book.best_bid(), Some(99));
        assert_eq!(book.best_ask(), Some(101));
    }

    #[test]
    fn same_price_fills_in_time_order() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Sell, 100, 5);
        book.limit(2, Side::Sell, 100, 5);
        let fills = book.market(3, Side::Buy, 7);
        assert_eq!(fills.len(), 2);
        assert_eq!((fills[0].maker, fills[0].qty), (1, 5));
        assert_eq!((fills[1].maker, fills[1].qty), (2, 2));
        assert_eq!(book.depth(Side::Sell, 100), 3);
    }

    #[test]
    fn better_price_fills_first() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Sell, 102, 5);
        book.limit(2, Side::Sell, 101, 5);
        let fills = book.market(3, Side::Buy, 5);
        assert_eq!((fills[0].maker, fills[0].price), (2, 101));
    }

    #[test]
    fn crossing_limit_trades_then_rests_remainder() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Sell, 100, 5);
        book.limit(2, Side::Sell, 102, 5);
        let fills = book.limit(3, Side::Buy, 101, 8);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].qty, 5);
        // 3 left over rests at 101 and does not trade through 102.
        assert_eq!(book.best_bid(), Some(101));
        assert_eq!(book.depth(Side::Buy, 101), 3);
        assert_eq!(book.best_ask(), Some(102));
    }

    #[test]
    fn cancel_removes_order_and_empty_level() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Buy, 99, 10);
        assert!(book.cancel(1));
        assert!(!book.cancel(1));
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn market_order_larger_than_book_is_partially_filled() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Buy, 99, 4);
        let fills = book.market(2, Side::Sell, 10);
        assert_eq!(fills.iter().map(|f| f.qty).sum::<Qty>(), 4);
        assert_eq!(book.best_bid(), None);
    }

    #[test]
    fn fully_filled_makers_leave_the_index() {
        let mut book = OrderBook::new();
        book.limit(1, Side::Sell, 100, 5);
        book.market(2, Side::Buy, 5);
        assert_eq!(book.order_count(), 0);
        assert!(!book.cancel(1));
    }

    #[test]
    fn book_never_stays_crossed() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};

        let mut rng = StdRng::seed_from_u64(3);
        let mut book = OrderBook::new();
        for id in 0..10_000 {
            let side = if rng.gen_bool(0.5) { Side::Buy } else { Side::Sell };
            book.limit(id, side, rng.gen_range(90..=110), rng.gen_range(1..=10));
            if let (Some(b), Some(a)) = (book.best_bid(), book.best_ask()) {
                assert!(b < a, "crossed book: bid {b} >= ask {a}");
            }
        }
    }
}
