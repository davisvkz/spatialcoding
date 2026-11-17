mod config;
mod domain;
mod net;
mod util;

use crate::domain::{Order, Product};
use crate::net::client::Client;
use crate::net::server::Server;

fn main() {
    let cfg = config::Config::load();
    let server = Server::bind(&cfg);
    let client = Client::new(server.addr());

    let product = Product::new(1, "keyboard", 4_900);
    let mut order = Order::new(7);
    order.add(product, 2);

    println!("{}", util::shout(&client.send(&order)));
}
