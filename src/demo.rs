//! The bundled demo project: playbooks against localhost that emit dost events.

use std::fs;
use std::path::Path;

use anyhow::Result;

const FILES: &[(&str, &str)] = &[
    ("ansible.cfg", include_str!("../assets/demo/ansible.cfg")),
    (
        "inventory/hosts.ini",
        include_str!("../assets/demo/inventory/hosts.ini"),
    ),
    (
        "playbooks/perimeter.yml",
        include_str!("../assets/demo/playbooks/perimeter.yml"),
    ),
    (
        "playbooks/vpn.yml",
        include_str!("../assets/demo/playbooks/vpn.yml"),
    ),
    (
        "playbooks/broken.yml",
        include_str!("../assets/demo/playbooks/broken.yml"),
    ),
    (
        ".dost/catalog/perimeter.yml",
        include_str!("../assets/demo/dost/catalog/perimeter.yml"),
    ),
    (
        ".dost/catalog/vpn.yml",
        include_str!("../assets/demo/dost/catalog/vpn.yml"),
    ),
    (
        ".dost/catalog/broken.yml",
        include_str!("../assets/demo/dost/catalog/broken.yml"),
    ),
    (
        ".dost/catalog/chains/demo.yml",
        include_str!("../assets/demo/dost/catalog/chains/demo.yml"),
    ),
    (
        ".dost/errors.yml",
        include_str!("../assets/demo/dost/errors.yml"),
    ),
];

/// Write the demo project into `dest` (which should be empty).
pub fn extract(dest: &Path) -> Result<()> {
    for (rel, body) in FILES {
        let path = dest.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, body)?;
    }
    Ok(())
}
