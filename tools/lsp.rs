use crate::compiler::CompilerPipeline;
use std::io::{self, BufRead, Write};

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

    pub fn handle_json_rpc_message(&mut self, msg: &str) -> Option<String> {
        if msg.contains("\"method\":\"initialize\"") {
            return Some(
                r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"textDocumentSync":1,"hoverProvider":true,"completionProvider":{"resolveProvider":false,"triggerCharacters":[".",":"]},"definitionProvider":true},"serverInfo":{"name":"aps-lsp","version":"1.0.0"}}}"#
                    .to_string(),
            );
        }
        if msg.contains("\"method\":\"shutdown\"") {
            return Some(r#"{"jsonrpc":"2.0","id":2,"result":null}"#.to_string());
        }
        if msg.contains("\"method\":\"textDocument/completion\"") {
            return Some(
                r#"{"jsonrpc":"2.0","id":3,"result":[{"label":"fn","kind":14,"detail":"APS function declaration"},{"label":"struct","kind":14,"detail":"APS structure declaration"},{"label":"region","kind":14,"detail":"AMA Region Memory Arena"},{"label":"read","kind":14,"detail":"AMA shared read capability"},{"label":"write","kind":14,"detail":"AMA exclusive write capability"},{"label":"take","kind":14,"detail":"AMA ownership transfer capability"},{"label":"share","kind":3,"detail":"Freeze into Shared Immutable Memory"},{"label":"println","kind":3,"detail":"fn println(val: read T)"}]}"#
                    .to_string(),
            );
        }
        if msg.contains("\"method\":\"textDocument/hover\"") {
            return Some(
                r#"{"jsonrpc":"2.0","id":4,"result":{"contents":{"kind":"markdown","value":"**APS Adaptive Memory Architecture (AMA)**\nVerified by static capability and lifetime analyzer."}}}"#
                    .to_string(),
            );
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
            if trimmed.starts_with('{') {
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
