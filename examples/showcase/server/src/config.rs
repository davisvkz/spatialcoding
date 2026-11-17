const DEFAULT: &str = include_str!("../config/default.toml");

pub struct Config {
    pub port: u16,
    pub host: String,
}

impl Config {
    pub fn load() -> Config {
        let port = DEFAULT
            .lines()
            .find_map(|l| l.strip_prefix("port = "))
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);
        Config { port, host: "127.0.0.1".into() }
    }
}
