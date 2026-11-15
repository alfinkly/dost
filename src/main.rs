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

fn require_ansible() -> Result<()> {
    doctor::check_ansible()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

async fn real_main() -> Result<i32> {
    let cli = Cli::parse();
    let tty = std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    match cli.command {
        Some(Cmd::Doctor) => {
            let mut bad = false;
            match doctor::check_ansible() {
                Ok(info) => {
                    println!(
                        "ansible-core {} at {}: ok",
                        info.version,
                        info.path.display()
                    );
                    match doctor::check_plugin_loads() {
                        Ok(()) => println!("callback dost_stream loads: ok"),
                        Err(e) => {
                            bad = true;
                            eprintln!("doctor: callback dost_stream does not load: {e}");
                        }
                    }
                }
                Err(e) => {
                    bad = true;
                    eprintln!("doctor: {e}");
                }
            }
            if let Some(root) = find_root(&std::env::current_dir()?) {
                let problems = doctor::validate_project(&root);
                if problems.is_empty() {
                    println!("project {}: ok", root.display());
                }
                for p in problems {
                    bad = true;
                    eprintln!("doctor: {p}");
                }
            }
            Ok(i32::from(bad))
        }
        Some(Cmd::Run { id, apply, confirm }) => {
            require_ansible()?;
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
            require_ansible()?;
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
        Some(Cmd::Init) => {
            let report = dost::init::init(&std::env::current_dir()?)?;
            for p in &report.created {
                println!("created {}", p.display());
            }
            for p in &report.skipped {
                println!("kept    {} (already exists)", p.display());
            }
            println!("Fill in `about:` and `done:` in .dost/catalog/*.yml, then run `dost`.");
            Ok(0)
        }
        Some(Cmd::Demo) => {
            if !tty {
                bail!("the demo needs a terminal");
            }
            require_ansible()?;
            let dir = tempfile::tempdir()?;
            dost::demo::extract(dir.path())?;
            let project = load_project(dir.path())?;
            app::run(project, Store::open(dir.path()), None).await?;
            Ok(0)
        }
        None => {
            require_ansible()?;
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
