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
    let include_local_first_starter = matches!(preset, Preset::LocalFirst);

    // Root workspace files
    fs::create_dir_all(&root)?;
    fs::write(root.join("Cargo.toml"), templates::workspace_cargo_toml())?;
    fs::write(
        root.join("galeon.toml"),
        templates::galeon_toml(name, preset_str),
    )?;
    fs::write(root.join(".gitignore"), templates::project_gitignore())?;
    if include_local_first_starter {
        fs::write(
            root.join("package.json"),
            templates::local_first_package_json(name),
        )?;
        fs::write(
            root.join("README.md"),
            templates::local_first_readme_md(name),
        )?;
    }

    // client/ web starter (local-first) or placeholder
    let client_dir = root.join("client");
    fs::create_dir_all(&client_dir)?;
    if include_local_first_starter {
        let client_src = client_dir.join("src");
        fs::create_dir_all(&client_src)?;
        fs::write(
            client_dir.join("tsconfig.json"),
            templates::local_first_client_tsconfig_json(),
        )?;
        fs::write(
            client_dir.join("index.html"),
            templates::local_first_client_index_html(name),
        )?;
        fs::write(
            client_src.join("main.ts"),
            templates::local_first_client_main_ts(),
        )?;
        fs::write(
            client_src.join("style.css"),
            templates::local_first_client_style_css(),
        )?;
    } else {
        fs::write(client_dir.join(".gitkeep"), "")?;
    }

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
    if include_local_first_starter {
        fs::write(
            domain_src.join("lib.rs"),
            templates::local_first_domain_lib_rs(),
        )?;
    } else {
        fs::write(domain_src.join("lib.rs"), templates::domain_lib_rs(name))?;
    }

    // crates/client (local-first)
    if include_local_first_starter {
        let wasm_src = root.join("crates").join("client").join("src");
        fs::create_dir_all(&wasm_src)?;
        fs::write(
            root.join("crates").join("client").join("Cargo.toml"),
            templates::local_first_client_cargo_toml(name),
        )?;
        fs::write(
            wasm_src.join("lib.rs"),
            templates::local_first_client_lib_rs(name),
        )?;
    }

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

        assert_file(&root, ".gitignore");
        assert_file(&root, "Cargo.toml");
        assert_file(&root, "galeon.toml");
        assert_file(&root, "package.json");
        assert_file(&root, "README.md");
        assert_file(&root, "client/index.html");
        assert_file(&root, "client/tsconfig.json");
        assert_file(&root, "client/src/main.ts");
        assert_file(&root, "client/src/style.css");
        assert_file(&root, "crates/protocol/Cargo.toml");
        assert_file(&root, "crates/protocol/src/lib.rs");
        assert_file(&root, "crates/domain/Cargo.toml");
        assert_file(&root, "crates/domain/src/lib.rs");
        assert_file(&root, "crates/client/Cargo.toml");
        assert_file(&root, "crates/client/src/lib.rs");

        assert_no_file(&root, "crates/server/Cargo.toml");
        assert_no_file(&root, "crates/db/Cargo.toml");
        assert_no_file(&root, "docker-compose.yml");
        assert_no_file(&root, "client/.gitkeep");

        let package_json = fs::read_to_string(root.join("package.json")).unwrap();
        assert!(package_json.contains(r#""dev": "bun run wasm && vite client""#));
        assert!(package_json.contains(r#""build": "bun run wasm && vite build client""#));

        let readme = fs::read_to_string(root.join("README.md")).unwrap();
        assert!(readme.contains("bun run dev"));
        assert!(readme.contains("bun run build"));

        let domain = fs::read_to_string(
            root.join("crates")
                .join("domain")
                .join("src")
                .join("lib.rs"),
        )
        .unwrap();
        assert!(domain.contains("StarterPlugin"));

        let wasm_client = fs::read_to_string(
            root.join("crates")
                .join("client")
                .join("src")
                .join("lib.rs"),
        )
        .unwrap();
        assert!(wasm_client.contains("StarterWasmEngine"));
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
        let local_first_pkg = templates::local_first_package_json("testgame");
        let local_first_client = templates::local_first_client_cargo_toml("testgame");
        let galeon_version = templates::galeon_release_version();
        let galeon_minor = templates::galeon_minor_version();

        for (label, content) in [
            ("protocol", &protocol),
            ("domain", &domain),
            ("server", &server),
        ] {
            assert!(
                content.contains(&format!(r#"galeon-engine = "{galeon_version}""#)),
                "{label} template missing published crate dependency"
            );
            assert!(
                !content.contains("galeon-engine/galeon.git"),
                "{label} template still references git URL"
            );
        }

        assert!(
            local_first_client.contains(&format!(r#"galeon-engine = "{galeon_version}""#)),
            "local-first client template missing engine dependency pinned to CLI release"
        );
        assert!(
            local_first_client
                .contains(&format!(r#"galeon-engine-three-sync = "{galeon_version}""#)),
            "local-first client template missing three-sync dependency pinned to CLI release"
        );
        assert!(
            local_first_pkg.contains(&format!(r#""@galeon/engine-ts": "^{galeon_version}""#)),
            "local-first package.json missing engine-ts dependency pinned to CLI release"
        );
        assert!(
            templates::galeon_toml("testgame", "local-first")
                .contains(&format!(r#"engine = "{galeon_minor}""#)),
            "galeon.toml should record the CLI major.minor engine line"
        );
    }
}
