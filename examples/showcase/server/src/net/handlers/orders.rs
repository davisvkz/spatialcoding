use super::health::ping;
use crate::domain::Order;

pub fn create(id: u64) -> (Order, &'static str) {
    (Order::new(id), ping())
}
