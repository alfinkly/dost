mod cli;
mod doctor;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    let code = match cli.command {
        Some(cli::Cmd::Doctor) => match doctor::check_ansible() {
            Ok(info) => {
                println!("ansible-core {} at {}", info.version, info.path.display());
                0
            }
            Err(e) => {
                eprintln!("doctor: {e}");
                1
            }
        },
        _ => {
            eprintln!("not implemented yet");
            2
        }
    };
    std::process::exit(code);
}
