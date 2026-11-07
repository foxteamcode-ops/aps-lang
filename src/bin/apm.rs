use aps::tools::apm::run_apm_command;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cwd = env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match run_apm_command(&args, &cwd) {
        Ok(msg) => println!("{}", msg),
        Err(err) => {
            eprintln!("error: {}", err);
            process::exit(1);
        }
    }
}
