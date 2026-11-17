use super::entity::{Entity, Priced};
use super::price::{with_tax, Cents};
use super::product::Product;

pub struct Order {
    id: u64,
    lines: Vec<(Product, u32)>,
}

impl Order {
    pub fn new(id: u64) -> Order {
        Order { id, lines: Vec::new() }
    }

    pub fn add(&mut self, product: Product, quantity: u32) {
        self.lines.push((product, quantity));
    }

    pub fn total(&self) -> Cents {
        let net: Cents = self.lines.iter().map(|(p, q)| p.price() * *q as u64).sum();
        with_tax(net, 21)
    }
}

impl Entity for Order {
    fn id(&self) -> u64 {
        self.id
    }
}
