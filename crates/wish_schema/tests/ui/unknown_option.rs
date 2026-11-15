use wish_schema::component;

#[component]
pub struct Health {
    #[schema(units = "hp")]
    pub value: f32,
}

fn main() {}
