use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(file: &str, line: usize, col: usize, start: usize, end: usize) -> Self {
        Self {
            file: file.to_string(),
            line,
            col,
            start,
            end,
        }
    }

    pub fn dummy() -> Self {
        Self {
            file: "<input>".to_string(),
            line: 1,
            col: 1,
            start: 0,
            end: 1,
        }
    }

    pub fn len(&self) -> usize {
        if self.end > self.start {
            self.end - self.start
        } else {
            1
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Note => write!(f, "note"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub label: Option<String>,
    pub help: Option<String>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Error,
            message: message.into(),
            span,
            label: None,
            help: None,
            notes: Vec::new(),
        }
    }

    pub fn warning(code: &str, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Warning,
            message: message.into(),
            span,
            label: None,
            help: None,
            notes: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn render(&self, source: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}[{}]: {}\n\n", self.severity, self.code, self.message));
        out.push_str(&format!(
            "  --> {}:{}:{}\n\n",
            self.span.file, self.span.line, self.span.col
        ));

        let lines: Vec<&str> = source.lines().collect();
        if self.span.line >= 1 && self.span.line <= lines.len() {
            let line_str = lines[self.span.line - 1];
            let line_num_str = format!("{}", self.span.line);
            let pad = " ".repeat(line_num_str.len());
            out.push_str(&format!("{} | {}\n", line_num_str, line_str));
            let col_offset = self.span.col.saturating_sub(1);
            let caret_len = self.span.len().max(1).min(line_str.len().saturating_sub(col_offset).max(1));
            let carets = "^".repeat(caret_len);
            if let Some(ref lbl) = self.label {
                out.push_str(&format!(
                    "{} | {}{} {}\n",
                    pad,
                    " ".repeat(col_offset),
                    carets,
                    lbl
                ));
            } else {
                out.push_str(&format!("{} | {}{}\n", pad, " ".repeat(col_offset), carets));
            }
        }

        for note in &self.notes {
            out.push_str(&format!("\nnote: {}\n", note));
        }

        if let Some(ref help) = self.help {
            out.push_str(&format!("\nhelp: {}\n", help));
        }

        out
    }

    pub fn to_json(&self) -> String {
        let escaped_msg = self.message.replace('\\', "\\\\").replace('"', "\\\"");
        let escaped_file = self.span.file.replace('\\', "\\\\").replace('"', "\\\"");
        let escaped_help = self
            .help
            .as_deref()
            .unwrap_or("")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!(
            "{{\"code\":\"{}\",\"severity\":\"{}\",\"message\":\"{}\",\"file\":\"{}\",\"line\":{},\"col\":{},\"help\":\"{}\"}}",
            self.code,
            self.severity,
            escaped_msg,
            escaped_file,
            self.span.line,
            self.span.col,
            escaped_help
        )
    }
}

#[derive(Debug, Default, Clone)]
pub struct DiagnosticBag {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub fn push(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn render_all(&self, source: &str) -> String {
        let mut s = String::new();
        for d in &self.diagnostics {
            s.push_str(&d.render(source));
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_render_e0042() {
        let src = "fn update(driver: read Driver) {\n    driver.balance += 100\n}\n";
        let span = Span::new("src/main.aps", 2, 5, 37, 51);
        let diag = Diagnostic::error("E0042", "cannot modify a read-only value", span)
            .with_label("cannot modify through read access")
            .with_help("request exclusive write access to this value");

        let rendered = diag.render(src);
        assert!(rendered.contains("error[E0042]: cannot modify a read-only value"));
        assert!(rendered.contains("--> src/main.aps:2:5"));
        assert!(rendered.contains("^^^^^^^^^^^^^^ cannot modify through read access"));
        assert!(rendered.contains("help: request exclusive write access to this value"));
    }
}
