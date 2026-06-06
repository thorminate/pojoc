use super::types::FieldId;

#[derive(Debug)]
pub struct IdGen {
    next: u64,
}

impl IdGen {
    pub fn new() -> Self {
        Self { next: 1 }
    }

    pub fn next(&mut self) -> FieldId {
        let id = self.next;
        self.next += 1;
        FieldId(id)
    }
}