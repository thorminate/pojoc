#[derive(Debug)]
pub enum Type {
    Named(String),
    Array(Box<Type>),
    // add more later
}