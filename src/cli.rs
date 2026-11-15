use clap::{Parser, Subcommand};

/// Terminal player for Ansible.
#[derive(Parser, Debug)]
#[command(name = "dost", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run one catalog command by id
    Run { id: String },
    /// Run a chain of commands by id
    Chain { id: String },
    /// Show progress of a chain
    Status { chain: String },
    /// Show past runs
    History,
    /// Create catalog stubs from the playbooks in this directory
    Init,
    /// Play the bundled demo (no servers needed)
    Demo,
    /// Check Ansible, the catalog and the callback plugin
    Doctor,
}
