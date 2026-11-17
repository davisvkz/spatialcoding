pub type Cents = u64;

pub fn format_cents(value: Cents) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}

pub fn with_tax(value: Cents, rate_percent: u64) -> Cents {
    value + value * rate_percent / 100
}
