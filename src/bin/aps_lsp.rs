use aps::tools::lsp::ApsLanguageServer;
use std::env;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("--version") {
        println!("aps-lsp {} (APS Language Server Protocol)", aps::APS_VERSION_DISPLAY);
        return;
    }
    let mut server = ApsLanguageServer::new();
    let _ = server.run_stdio_loop();
}
