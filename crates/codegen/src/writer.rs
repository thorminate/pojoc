#[derive(Default)]
pub struct CodeWriter {
    buf: String,
    indent: usize,
}

impl CodeWriter {
    pub fn line(&mut self, s: &str) {
        let indent = "    ".repeat(self.indent);
        self.buf.push_str(&indent);
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    pub fn blank(&mut self) {
        self.buf.push('\n');
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    pub fn finish(self) -> String {
        self.buf
    }
}
