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
    Run {
        id: String,
        /// Without a terminal: apply after a passed check (a check always runs first)
        #[arg(long)]
        apply: bool,
        /// Without a terminal: the command id, required to apply a `danger: high` command
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Run the next unfinished step of a chain
    Chain {
        id: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        confirm: Option<String>,
    },
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
