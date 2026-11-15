use wish_schema::schema;

#[schema]
pub enum Shape {
    Sphere,
    Box(f32),
}

fn main() {}
