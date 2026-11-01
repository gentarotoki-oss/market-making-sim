//! A price-level (L2) order book: total size at each price, as published by
//! exchange depth feeds. Unlike `orderbook::OrderBook` it knows nothing about
//! individual orders; an update simply sets the size at a level, and a size of
//! zero removes the level.

use std::collections::BTreeMap;

use crate::orderbook::{Price, Side};

/// Size in exchange lots (for BTCUSDT futures, 1 lot = 0.001 BTC).
pub type Lots = u64;

#[derive(Debug, Default, Clone)]
pub struct L2Book {
    bids: BTreeMap<Price, Lots>,
    asks: BTreeMap<Price, Lots>,
}

impl L2Book {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the size at a level; zero removes it.
    pub fn set(&mut self, side: Side, price: Price, size: Lots) {
        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        if size == 0 {
            book.remove(&price);
        } else {
            book.insert(price, size);
        }
    }

    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }

    pub fn best_bid(&self) -> Option<(Price, Lots)> {
        self.bids.iter().next_back().map(|(&p, &q)| (p, q))
    }

    pub fn best_ask(&self) -> Option<(Price, Lots)> {
        self.asks.iter().next().map(|(&p, &q)| (p, q))
    }

    pub fn is_crossed(&self) -> bool {
        matches!((self.best_bid(), self.best_ask()), (Some((b, _)), Some((a, _))) if b >= a)
    }

    pub fn levels(&self, side: Side) -> usize {
        match side {
            Side::Buy => self.bids.len(),
            Side::Sell => self.asks.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_remove_levels() {
        let mut book = L2Book::new();
        book.set(Side::Buy, 100, 5);
        book.set(Side::Buy, 101, 3);
        book.set(Side::Sell, 103, 7);
        assert_eq!(book.best_bid(), Some((101, 3)));
        assert_eq!(book.best_ask(), Some((103, 7)));

        book.set(Side::Buy, 101, 0);
        assert_eq!(book.best_bid(), Some((100, 5)));
        assert_eq!(book.levels(Side::Buy), 1);
    }

    #[test]
    fn update_replaces_size() {
        let mut book = L2Book::new();
        book.set(Side::Sell, 103, 7);
        book.set(Side::Sell, 103, 2);
        assert_eq!(book.best_ask(), Some((103, 2)));
    }

    #[test]
    fn detects_crossed_book() {
        let mut book = L2Book::new();
        book.set(Side::Buy, 103, 1);
        book.set(Side::Sell, 103, 1);
        assert!(book.is_crossed());
    }
}
