use std::collections::HashMap;

#[derive(Default)]
pub struct CodeWriter {
    buf: String,
    pub indent: usize,
}

impl CodeWriter {
    pub fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.buf.push_str("    ")
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    pub fn write(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.buf.push_str("    ")
        }
        self.buf.push_str(s);
    }

    pub fn blank(&mut self) {
        self.buf.push('\n');
    }

    pub(super) fn append(&mut self, code: &str) {
        self.buf.push_str(code);
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

/// Group version implementations by their complete emitted body.
#[derive(Default)]
pub(super) struct VersionBodies(HashMap<String, Vec<i128>>);

impl VersionBodies {
    pub(super) fn insert(&mut self, version: i128, body: CodeWriter) {
        self.0.entry(body.finish()).or_default().push(version);
    }

    pub(super) fn into_groups(self) -> Vec<(String, Vec<i128>)> {
        let mut groups: Vec<_> = self.0.into_iter().collect();
        groups.sort_by_key(|(_, versions)| versions[0]);
        groups
    }
}

pub(super) fn version_pattern(versions: &[i128]) -> String {
    let mut patterns = Vec::new();
    let mut versions = versions.iter().copied().peekable();
    while let Some(first) = versions.next() {
        let mut last = first;
        while let Some(&next) = versions.peek() {
            if last.checked_add(1) != Some(next) {
                break;
            }
            last = next;
            versions.next();
        }
        patterns.push(if first == last {
            first.to_string()
        } else {
            format!("{first}..={last}")
        });
    }
    patterns.join(" | ")
}
