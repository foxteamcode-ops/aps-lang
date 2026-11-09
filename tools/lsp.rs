use crate::compiler::CompilerPipeline;
use std::io::{self, BufRead, Read, Write};

pub struct ApsLanguageServer {
    pub last_uri: String,
    pub last_source: String,
}

impl ApsLanguageServer {
    pub fn new() -> Self {
        Self {
            last_uri: String::new(),
            last_source: String::new(),
        }
    }

    fn extract_id(msg: &str) -> String {
        if let Some(pos) = msg.find("\"id\":") {
            let rest = msg[pos + 5..].trim_start();
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            rest[..end].trim().to_string()
        } else {
            "1".to_string()
        }
    }

    pub fn handle_json_rpc_message(&mut self, msg: &str) -> Option<String> {
        let req_id = Self::extract_id(msg);

        if msg.contains("\"method\":\"initialize\"") {
            return Some(format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":{{"capabilities":{{"textDocumentSync":1,"hoverProvider":true,"completionProvider":{{"resolveProvider":false,"triggerCharacters":[".",":"]}},"definitionProvider":true}},"serverInfo":{{"name":"aps-lsp","version":"0.1.0-alpha"}}}}}}"#,
                req_id
            ));
        }
        if msg.contains("\"method\":\"shutdown\"") {
            return Some(format!(r#"{{"jsonrpc":"2.0","id":{},"result":null}}"#, req_id));
        }
        if msg.contains("\"method\":\"textDocument/completion\"") {
            return Some(format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":[{{"label":"fn","kind":14,"detail":"APS function declaration"}},{{"label":"struct","kind":14,"detail":"APS structure declaration"}},{{"label":"region","kind":14,"detail":"AMA Region Memory Arena"}},{{"label":"read","kind":14,"detail":"AMA shared read capability"}},{{"label":"write","kind":14,"detail":"AMA exclusive write capability"}},{{"label":"take","kind":14,"detail":"AMA ownership transfer capability"}},{{"label":"share","kind":3,"detail":"Freeze into Shared Immutable Memory"}},{{"label":"println","kind":3,"detail":"fn println(val: read T)"}}]}}"#,
                req_id
            ));
        }
        if msg.contains("\"method\":\"textDocument/hover\"") {
            return Some(format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":{{"contents":{{"kind":"markdown","value":"**APS Adaptive Memory Architecture (AMA)**\nVerified by static capability and lifetime analyzer."}}}}}}"#,
                req_id
            ));
        }
        if msg.contains("\"method\":\"textDocument/didOpen\"")
            || msg.contains("\"method\":\"textDocument/didChange\"")
        {
            if let Some(uri_start) = msg.find("\"uri\":\"") {
                let rest = &msg[uri_start + 7..];
                if let Some(uri_end) = rest.find('"') {
                    let uri = &rest[..uri_end];
                    self.last_uri = uri.to_string();
                    if let Some(text_start) = msg.find("\"text\":\"") {
                        let text_rest = &msg[text_start + 8..];
                        if let Some(text_end) = text_rest.find("\"}") {
                            let source = &text_rest[..text_end];
                            self.last_source = source.to_string();
                            return Some(self.analyze_document_to_lsp_diagnostics(uri, source));
                        }
                    }
                }
            }
        }
        None
    }

    pub fn analyze_document_to_lsp_diagnostics(&self, filename: &str, source: &str) -> String {
        let pipeline = CompilerPipeline::new(0);
        let res = pipeline.check_source(filename, source);
        let items: Vec<String> = res
            .diagnostics
            .diagnostics
            .iter()
            .map(|d| d.to_json())
            .collect();
        format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":\"{}\",\"diagnostics\":[{}]}}}}",
            filename,
            items.join(",")
        )
    }

    pub fn run_stdio_loop(&mut self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        let mut reader = stdin.lock();
        let mut line = String::new();

        loop {
            line.clear();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("Content-Length:") {
                let len_str = trimmed.trim_start_matches("Content-Length:").trim();
                if let Ok(content_len) = len_str.parse::<usize>() {
                    let mut empty_line = String::new();
                    reader.read_line(&mut empty_line)?;
                    let mut body_buf = vec![0u8; content_len];
                    reader.read_exact(&mut body_buf)?;
                    if let Ok(body_str) = std::str::from_utf8(&body_buf) {
                        if let Some(resp) = self.handle_json_rpc_message(body_str) {
                            let packet = format!("Content-Length: {}\r\n\r\n{}", resp.len(), resp);
                            stdout.write_all(packet.as_bytes())?;
                            stdout.flush()?;
                        }
                    }
                }
            } else if trimmed.starts_with('{') {
                if let Some(resp) = self.handle_json_rpc_message(trimmed) {
                    let packet = format!("Content-Length: {}\r\n\r\n{}", resp.len(), resp);
                    stdout.write_all(packet.as_bytes())?;
                    stdout.flush()?;
                }
            }
        }
        Ok(())
    }
}
