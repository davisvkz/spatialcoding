use crate::config::Config;

pub struct Server {
    port: u16,
}

impl Server {
    pub fn bind(config: &Config) -> Server {
        Server { port: config.port }
    }

    pub fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}
