use wish_schema::component;

#[component]
pub struct Health {
    #[schema(default = "full")]
    pub value: f32,
}

fn main() {}
