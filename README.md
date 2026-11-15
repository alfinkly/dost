# dost

**dost** is a terminal player for Ansible. Pick a playbook from a menu, watch what is
happening as a live scene (ports closing, a VPN chain building host by host), and read in
plain words what has changed when it finishes.

The project is named after the Russian writer Fyodor Dostoevsky (Достоевский).
The CLI command is `dost` ("dost" also means "friend" in Hindi and Urdu).
It is not affiliated with or endorsed by any person or organisation connected with the writer.

## Why

Ansible itself is fine. What gets lost is the human side: there are many playbooks, they
have an order, you forget what each one does, the same errors come back and you forget the
fix, and after a run it is not obvious what is now true. `dost` keeps that knowledge next to
your playbooks and shows it when you need it.

## What it does

- **Menu without flags.** Every playbook has a title and a "what is this for".
- **Check first, always.** `--check --diff` runs before anything is applied. Apply needs a
  confirmation; `danger: medium|high` commands need you to type their id.
- **Live scenes** built from small blocks (chain, table, map, list). Output of a check run is
  drawn dimmed and labelled "ПРОВЕРКА, предварительно" so a preview is never mistaken for a fact.
- **Plain-language summary** after the run, written by you in the catalog (`done:`).
- **Known errors** get their cause and fix shown on the spot.
- **Chains** of playbooks remember progress: "3 of 6 done".
- **Retry and help.** `r` retries a failed step, `p` retries with a new sudo password, `h` asks Claude (headless,
  read-only) for a patch or steps, and `a`/`Enter`/`c` use the answer.
- **Safe by construction.** The sudo password is typed into a masked field and handed to
  Ansible through an anonymous pipe: never in argv, environment, files, history or logs.

It runs locally on top of your regular `ansible-playbook`. It has no server, no agent and
touches nothing on its own.

## Status

Version 0.1, early. macOS and Linux. Needs ansible-core 2.15 or newer and a Rust toolchain to
build. Not published to crates.io yet.

## Install

```bash
git clone https://github.com/alfinkly/dost
cd dost
./install.sh        # builds a release and puts `dost` into ~/.local/bin
dost doctor
```

`PREFIX=/usr/local ./install.sh` installs elsewhere, `./install.sh --uninstall` removes it. The
script touches nothing but `PREFIX/bin/dost` (no shell config, no aliases); if that directory is
not in your `PATH` it prints the line to add.

## Try it without any servers

```bash
dost demo
```

The demo plays bundled playbooks against `localhost`; nothing is changed.

## Use it on your playbooks

```bash
cd your-ansible-project      # has a playbooks/ directory
dost init                    # creates .dost/catalog/*.yml stubs, never overwrites
dost doctor                  # checks Ansible, the catalog and the callback plugin
dost                         # the menu
```

Fill in `about:` and `done:` in the stubs. See [docs/quickstart.md](docs/quickstart.md),
[docs/events.md](docs/events.md) for the scenes' data and [docs/scenes.md](docs/scenes.md)
for writing your own.

Without a terminal (CI, a pipe) there is no menu: `dost run <id>` prints plain lines and runs
only the check; add `--apply` to apply (and `--confirm <id>` for `danger: high`). The sudo
password is then read from the first line of stdin.

## Design

[docs/design.en.md](docs/design.en.md) (English), [docs/design.md](docs/design.md) (original, Russian).

## License

Apache-2.0, see [LICENSE](LICENSE).
