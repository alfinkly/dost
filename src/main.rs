use std::io::IsTerminal;

use anyhow::{bail, Result};
use clap::Parser;
use dost::catalog::{find_root, load_project, Project};
use dost::cli::{Cli, Cmd};
use dost::store::Store;
use dost::{app, doctor, plain};

fn open_project() -> Result<(Project, Store)> {
    let cwd = std::env::current_dir()?;
    let Some(root) = find_root(&cwd) else {
        bail!(
            "no .dost directory found here or above; run `dost init` in your playbooks directory"
        );
    };
    let project = load_project(&root)?;
    let store = Store::open(&root);
    Ok((project, store))
}

async fn real_main() -> Result<i32> {
    let cli = Cli::parse();
    let tty = std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    match cli.command {
        Some(Cmd::Doctor) => match doctor::check_ansible() {
            Ok(info) => {
                println!("ansible-core {} at {}", info.version, info.path.display());
                Ok(0)
            }
            Err(e) => {
                eprintln!("doctor: {e}");
                Ok(1)
            }
        },
        Some(Cmd::Run { id, apply, confirm }) => {
            let (project, store) = open_project()?;
            if tty {
                app::run(project, store, Some(&id)).await?;
                Ok(0)
            } else {
                plain::run_plain(
                    &project,
                    &store,
                    &id,
                    apply,
                    confirm.as_deref(),
                    &mut std::io::stdout(),
                )
                .await
            }
        }
        Some(Cmd::Chain { id, apply, confirm }) => {
            let (project, store) = open_project()?;
            let Some(chain) = project.chains.iter().find(|c| c.id == id) else {
                bail!("no chain with id `{id}`");
            };
            let progress = store.chain_progress(chain, |step| {
                project
                    .commands
                    .iter()
                    .find(|c| c.id == step)
                    .map(|c| dost::store::playbook_hash(c, &project.root))
                    .unwrap_or_default()
            });
            let Some((next, _)) = progress
                .iter()
                .find(|(_, s)| *s != dost::store::StepStatus::Done)
            else {
                println!("Chain `{id}` is complete.");
                return Ok(0);
            };
            let next = next.clone();
            if tty {
                app::run(project, store, Some(&next)).await?;
                Ok(0)
            } else {
                plain::run_plain(
                    &project,
                    &store,
                    &next,
                    apply,
                    confirm.as_deref(),
                    &mut std::io::stdout(),
                )
                .await
            }
        }
        Some(Cmd::Status { chain }) => {
            let (project, store) = open_project()?;
            plain::print_status(&project, &store, &chain, &mut std::io::stdout())?;
            Ok(0)
        }
        Some(Cmd::History) => {
            let (_, store) = open_project()?;
            plain::print_history(&store, &mut std::io::stdout())?;
            Ok(0)
        }
        Some(Cmd::Init) | Some(Cmd::Demo) => {
            eprintln!("not implemented yet");
            Ok(2)
        }
        None => {
            let (project, store) = open_project()?;
            if !tty {
                bail!("dost needs a terminal for the menu; use `dost run <id>` in scripts");
            }
            app::run(project, store, None).await?;
            Ok(0)
        }
    }
}

#[tokio::main]
async fn main() {
    match real_main().await {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("dost: {e:#}");
            std::process::exit(1);
        }
    }
}
