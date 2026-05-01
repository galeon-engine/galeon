# CLI

## Scaffold Smoke Checks

Use this check after changing `galeon new` templates or dependency pins. It
scaffolds one project for each preset outside the repository and runs
`cargo check --workspace` in each generated workspace.

```bash
tmp_dir="$(mktemp -d)"
repo_cargo="$PWD/Cargo.toml"

for preset in local-first hybrid server-authoritative; do
  project="check-$preset"
  (
    cd "$tmp_dir"
    cargo run --manifest-path "$repo_cargo" -p galeon-cli -- new "$project" --preset "$preset"
  )
  (
    cd "$tmp_dir/$project"
    cargo check --workspace
  )
done
```
