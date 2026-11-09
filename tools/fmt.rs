pub fn format_aps_source(source: &str) -> String {
    let mut formatted = String::new();
    let mut indent_level = 0usize;

    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            formatted.push('\n');
            continue;
        }

        // Decrease indent if line starts with closing delimiter
        if trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')') {
            indent_level = indent_level.saturating_sub(1);
        }

        let indent = "    ".repeat(indent_level);
        // Strip redundant trailing semicolon on standard single-statement lines unless inside a comment
        let clean_line = if !trimmed.starts_with("//") && trimmed.ends_with(';') && !trimmed.contains('[') {
            trimmed.trim_end_matches(';').trim_end()
        } else {
            trimmed
        };

        formatted.push_str(&indent);
        formatted.push_str(clean_line);
        formatted.push('\n');

        // Count braces outside string literals, char literals, and // comments
        let mut in_str = false;
        let mut in_char = false;
        let mut escaped = false;
        let mut opens = 0usize;
        let mut closes = 0usize;

        let chars: Vec<char> = trimmed.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if c == '\\' && (in_str || in_char) {
                escaped = true;
                i += 1;
                continue;
            }
            if c == '"' && !in_char {
                in_str = !in_str;
                i += 1;
                continue;
            }
            if c == '\'' && !in_str {
                in_char = !in_char;
                i += 1;
                continue;
            }
            if !in_str && !in_char {
                if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    break;
                }
                if c == '{' {
                    opens += 1;
                } else if c == '}' {
                    closes += 1;
                }
            }
            i += 1;
        }

        let leading_close = usize::from(trimmed.starts_with('}'));

        if opens > closes.saturating_sub(leading_close) {
            indent_level += opens - closes.saturating_sub(leading_close);
        } else if closes > opens + leading_close {
            indent_level = indent_level.saturating_sub(closes - opens - leading_close);
        }
    }

    formatted
}
