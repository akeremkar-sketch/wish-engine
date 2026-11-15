use wish_schema::component;

#[component]
pub struct Health {
    #[schema(range = 0.0..100.0)]
    pub value: f32,
}

fn main() {}
