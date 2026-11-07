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

        // Adjust indent for next line based on net braces outside strings/comments
        let code_part = trimmed.split("//").next().unwrap_or(trimmed);
        let opens = code_part.chars().filter(|c| *c == '{').count();
        let closes = code_part.chars().filter(|c| *c == '}').count();
        let leading_close = usize::from(trimmed.starts_with('}'));

        if opens > closes.saturating_sub(leading_close) {
            indent_level += opens - closes.saturating_sub(leading_close);
        } else if closes > opens + leading_close {
            indent_level = indent_level.saturating_sub(closes - opens - leading_close);
        }
    }

    formatted
}
