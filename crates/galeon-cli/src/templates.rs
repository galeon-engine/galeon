// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/// Root `Cargo.toml` for the generated workspace.
pub fn workspace_cargo_toml() -> String {
    r#"[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
edition = "2024"
"#
    .to_owned()
}

/// `galeon.toml` project config.
pub fn galeon_toml(name: &str, preset: &str) -> String {
    format!(
        r#"[project]
name = "{name}"
engine = "0.1"
preset = "{preset}"
"#
    )
}

/// `crates/protocol/Cargo.toml`.
pub fn protocol_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}-protocol"
version = "0.1.0"
edition.workspace = true

[dependencies]
galeon-engine = "0.2.0"
"#
    )
}

/// `crates/protocol/src/lib.rs`.
pub fn protocol_lib_rs(name: &str) -> String {
    format!(
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Protocol definitions for {name}.
//!
//! Define your commands, queries, events, and DTOs here using
//! `#[galeon_engine::command]`, `#[galeon_engine::query]`, etc.
"#
    )
}

/// `crates/domain/Cargo.toml`.
pub fn domain_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}-domain"
version = "0.1.0"
edition.workspace = true

[dependencies]
galeon-engine = "0.2.0"
{name}-protocol = {{ path = "../protocol" }}
"#
    )
}

/// `crates/domain/src/lib.rs`.
pub fn domain_lib_rs(name: &str) -> String {
    format!(
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Game systems and handlers for {name}.
"#
    )
}

/// `crates/server/Cargo.toml` (server-authoritative and hybrid presets).
pub fn server_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}-server"
version = "0.1.0"
edition.workspace = true

[dependencies]
galeon-engine = "0.2.0"
{name}-protocol = {{ path = "../protocol" }}
{name}-domain = {{ path = "../domain" }}
axum = "0.8"
tokio = {{ version = "1", features = ["full"] }}
"#
    )
}

/// `crates/server/src/main.rs` (server-authoritative and hybrid presets).
pub fn server_main_rs(name: &str) -> String {
    format!(
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

fn main() {{
    println!("TODO: {name} server");
}}
"#
    )
}

/// `crates/db/Cargo.toml` (server-authoritative preset only).
pub fn db_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}-db"
version = "0.1.0"
edition.workspace = true

[dependencies]
sqlx = {{ version = "0.8", features = ["runtime-tokio-rustls", "postgres"] }}
"#
    )
}

/// `crates/db/src/lib.rs` (server-authoritative preset only).
pub fn db_lib_rs(name: &str) -> String {
    format!(
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Database migrations and queries for {name}.
"#
    )
}

/// `docker-compose.yml` (server-authoritative preset only).
pub fn docker_compose_yml(name: &str) -> String {
    format!(
        r#"services:
  postgres:
    image: postgres:17
    ports:
      - "5432:5432"
    environment:
      POSTGRES_DB: {name}
      POSTGRES_USER: {name}
      POSTGRES_PASSWORD: dev
    volumes:
      - pgdata:/var/lib/postgresql/data

volumes:
  pgdata:
"#
    )
}
