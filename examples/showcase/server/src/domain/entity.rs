pub trait Entity {
    fn id(&self) -> u64;

    fn describe(&self) -> String {
        format!("entity #{}", self.id())
    }
}

/// A supertrait: `Priced` refines `Entity`.
pub trait Priced: Entity {
    fn price(&self) -> u64;
}
