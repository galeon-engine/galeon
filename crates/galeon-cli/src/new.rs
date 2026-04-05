// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::fs;
use std::io;
use std::path::Path;

use crate::Preset;
use crate::templates;

/// Scaffold a new Galeon game project at `<base>/<name>/`.
///
/// `base` is the directory in which the project folder is created.
/// Pass `Path::new(".")` (or the current directory) for the typical CLI case.
pub fn scaffold(base: &Path, name: &str, preset: &Preset) -> Result<(), io::Error> {
    let root = base.join(name);

    let preset_str = match preset {
        Preset::ServerAuthoritative => "server-authoritative",
        Preset::LocalFirst => "local-first",
        Preset::Hybrid => "hybrid",
    };

    let include_server = matches!(preset, Preset::ServerAuthoritative | Preset::Hybrid);
    let include_db = matches!(preset, Preset::ServerAuthoritative);
    let include_docker = matches!(preset, Preset::ServerAuthoritative);

    // Root workspace files
    fs::create_dir_all(&root)?;
    fs::write(root.join("Cargo.toml"), templates::workspace_cargo_toml())?;
    fs::write(
        root.join("galeon.toml"),
        templates::galeon_toml(name, preset_str),
    )?;

    // client/ placeholder
    let client_dir = root.join("client");
    fs::create_dir_all(&client_dir)?;
    fs::write(client_dir.join(".gitkeep"), "")?;

    // crates/protocol
    let protocol_src = root.join("crates").join("protocol").join("src");
    fs::create_dir_all(&protocol_src)?;
    fs::write(
        root.join("crates").join("protocol").join("Cargo.toml"),
        templates::protocol_cargo_toml(name),
    )?;
    fs::write(
        protocol_src.join("lib.rs"),
        templates::protocol_lib_rs(name),
    )?;

    // crates/domain
    let domain_src = root.join("crates").join("domain").join("src");
    fs::create_dir_all(&domain_src)?;
    fs::write(
        root.join("crates").join("domain").join("Cargo.toml"),
        templates::domain_cargo_toml(name),
    )?;
    fs::write(domain_src.join("lib.rs"), templates::domain_lib_rs(name))?;

    // crates/server (server-authoritative + hybrid)
    if include_server {
        let server_src = root.join("crates").join("server").join("src");
        fs::create_dir_all(&server_src)?;
        fs::write(
            root.join("crates").join("server").join("Cargo.toml"),
            templates::server_cargo_toml(name),
        )?;
        fs::write(server_src.join("main.rs"), templates::server_main_rs(name))?;
    }

    // crates/db (server-authoritative only)
    if include_db {
        let db_src = root.join("crates").join("db").join("src");
        fs::create_dir_all(&db_src)?;
        fs::write(
            root.join("crates").join("db").join("Cargo.toml"),
            templates::db_cargo_toml(name),
        )?;
        fs::write(db_src.join("lib.rs"), templates::db_lib_rs(name))?;
    }

    // docker-compose.yml (server-authoritative only)
    if include_docker {
        fs::write(
            root.join("docker-compose.yml"),
            templates::docker_compose_yml(name),
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn run_scaffold(name: &str, preset: Preset) -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        scaffold(tmp.path(), name, &preset).unwrap();
        let project_root = tmp.path().join(name);
        (tmp, project_root)
    }

    fn assert_file(root: &PathBuf, rel: &str) {
        let p = root.join(rel);
        assert!(p.exists(), "expected file missing: {}", p.display());
    }

    fn assert_no_file(root: &PathBuf, rel: &str) {
        let p = root.join(rel);
        assert!(!p.exists(), "unexpected file present: {}", p.display());
    }

    #[test]
    fn test_scaffold_server_authoritative() {
        let (_tmp, root) = run_scaffold("testgame", Preset::ServerAuthoritative);

        assert_file(&root, "Cargo.toml");
        assert_file(&root, "galeon.toml");
        assert_file(&root, "client/.gitkeep");
        assert_file(&root, "crates/protocol/Cargo.toml");
        assert_file(&root, "crates/protocol/src/lib.rs");
        assert_file(&root, "crates/domain/Cargo.toml");
        assert_file(&root, "crates/domain/src/lib.rs");
        assert_file(&root, "crates/server/Cargo.toml");
        assert_file(&root, "crates/server/src/main.rs");
        assert_file(&root, "crates/db/Cargo.toml");
        assert_file(&root, "crates/db/src/lib.rs");
        assert_file(&root, "docker-compose.yml");
    }

    #[test]
    fn test_scaffold_local_first() {
        let (_tmp, root) = run_scaffold("localgame", Preset::LocalFirst);

        assert_file(&root, "Cargo.toml");
        assert_file(&root, "galeon.toml");
        assert_file(&root, "client/.gitkeep");
        assert_file(&root, "crates/protocol/Cargo.toml");
        assert_file(&root, "crates/protocol/src/lib.rs");
        assert_file(&root, "crates/domain/Cargo.toml");
        assert_file(&root, "crates/domain/src/lib.rs");

        assert_no_file(&root, "crates/server/Cargo.toml");
        assert_no_file(&root, "crates/db/Cargo.toml");
        assert_no_file(&root, "docker-compose.yml");
    }

    #[test]
    fn test_scaffold_hybrid() {
        let (_tmp, root) = run_scaffold("hybridgame", Preset::Hybrid);

        assert_file(&root, "Cargo.toml");
        assert_file(&root, "galeon.toml");
        assert_file(&root, "client/.gitkeep");
        assert_file(&root, "crates/protocol/Cargo.toml");
        assert_file(&root, "crates/protocol/src/lib.rs");
        assert_file(&root, "crates/domain/Cargo.toml");
        assert_file(&root, "crates/domain/src/lib.rs");
        assert_file(&root, "crates/server/Cargo.toml");
        assert_file(&root, "crates/server/src/main.rs");

        assert_no_file(&root, "crates/db/Cargo.toml");
        assert_no_file(&root, "docker-compose.yml");
    }

    #[test]
    fn test_galeon_toml_content() {
        let (_tmp, root) = run_scaffold("myproject", Preset::ServerAuthoritative);

        let content = fs::read_to_string(root.join("galeon.toml")).unwrap();
        assert!(
            content.contains("name = \"myproject\""),
            "galeon.toml missing name"
        );
        assert!(
            content.contains("preset = \"server-authoritative\""),
            "galeon.toml missing preset"
        );
    }
}

#[cfg(test)]
mod template_dep_tests {
    use crate::templates;

    #[test]
    fn scaffolded_deps_use_published_crate() {
        let protocol = templates::protocol_cargo_toml("testgame");
        let domain = templates::domain_cargo_toml("testgame");
        let server = templates::server_cargo_toml("testgame");

        for (label, content) in [
            ("protocol", &protocol),
            ("domain", &domain),
            ("server", &server),
        ] {
            assert!(
                content.contains(r#"galeon-engine = "0.2.0""#),
                "{label} template missing published crate dependency"
            );
            assert!(
                !content.contains("galeon-engine/galeon.git"),
                "{label} template still references git URL"
            );
        }
    }
}
