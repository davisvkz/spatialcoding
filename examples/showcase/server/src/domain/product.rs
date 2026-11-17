use super::entity::{Entity, Priced};
use super::price::Cents;

pub struct Product {
    id: u64,
    pub name: String,
    price: Cents,
}

impl Product {
    pub fn new(id: u64, name: &str, price: Cents) -> Product {
        Product { id, name: name.to_string(), price }
    }
}

impl Entity for Product {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Priced for Product {
    fn price(&self) -> u64 {
        self.price
    }
}
