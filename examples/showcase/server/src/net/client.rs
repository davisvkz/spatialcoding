use crate::domain::{Entity, Order};
use crate::util::cents;

pub struct Client {
    addr: String,
}

impl Client {
    pub fn new(addr: String) -> Client {
        Client { addr }
    }

    pub fn send(&self, order: &Order) -> String {
        format!("{} -> order {} total {}", self.addr, order.id(), cents(order.total()))
    }
}
