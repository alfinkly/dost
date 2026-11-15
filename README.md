# dost

**dost** is a terminal player for Ansible: pick a playbook from a menu, watch what is
happening as a live scene (ports closing, a VPN chain building host by host), and read in
plain words what has changed when it finishes.

The project is named after the Russian writer Fyodor Dostoevsky (Достоевский).
The CLI command is `dost` ("dost" also means "friend" in Hindi and Urdu).
It is not affiliated with or endorsed by any person or organisation connected with the writer.

## Status

Early design. Nothing here is released or implemented yet. The design lives in
[docs/design.md](docs/design.md) (currently in Russian).

## Idea

- A menu of your playbooks with a short "what is this for", no flags to remember.
- A live scene while the playbook runs, built from small visual blocks (chain, table, map, graph).
- A plain-language summary at the end: what is true now.
- Known errors get a cause and a fix on the spot.
- Chains of playbooks remember progress: "3 of 6 done".
- Written in Rust, runs locally on top of the regular `ansible-playbook`.

## License

Apache-2.0, see [LICENSE](LICENSE).
