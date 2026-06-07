# https://vaie.art/ service map & deployment
Central location for managing, deploying, and updating separate services running on https://vaie.art/

This repository exposes a CLI that:
1. Takes a number of local Git repositories or submodules (each one hosting server code like Node.js servers, SvelteKit projects, etc.)
    1. ...and keeps them up to date when prompted
1. Based on per-service configuration you provide in `services.toml`:
    1. Autogenerates a `Caddyfile` web routing config
    1. Autogenerates a `systemd` config for each service, for concurrently running all the services
    1. Builds each project using a set of commands
    1. Uploads the `Caddyfile`, `systemd`, and build artifacts to a remote server
        1. ...and cleans up stale builds/configs from previous runs

Outside of work this repository does, you will still need to manually consider:
1. Initial setup of the environment on the remote server
1. DNS and nameserver setup on the domain registrar or any proxies

## Workflow
1. Clone any new repositories to build services from under `./src/submodules/`
1. Configure service locations, build commands, build types, etc. in `services.toml`
1. Configure private SSH connection details in `services.local.toml` (gitignored)
1. `cargo run -- check` to validate the service map
1. `cargo run -- update` to pull latest versions of the listed submodules
1. `cargo run -- render` to generate a `Caddyfile` and `systemd` configs
1. `cargo run -- plan` to view the remote build, rsync, validation, and install commands
1. `cargo run -- deploy` to build configured repositories, rsync files to a remote temp directory, validate Caddy, remove stale artifacts and config files on the server, and restart changed services

## Commands
- `check`: validate config, local repo paths, ports, routes, and generated templates
- `update`: run `git fetch --all --prune` and `git pull --ff-only` for each configured repo
- `render`: write generated artifacts to `target/vaieart-services/`
- `plan`: print the deployment command sequence without running it
- `deploy`: render artifacts, run local build commands, upload with `rsync`, validate Caddy, and apply changes over SSH.
- `deploy --dry-run`: print the same command sequence as `plan`

## Dependencies
CLI needs `git`, `ssh`, `rsync`, `cargo`

Configured build commands also need their local tools. For static Svelte apps, `deno` must resolve on `PATH`; in WSL this can be either Linux `deno` or Windows `deno.exe` through interop

Windows: consider using WSL with one of the below distros

Fedora:
```sh
sudo dnf install -y git rsync openssh-clients
```

Debian/Ubuntu:
```sh
sudo apt update
sudo apt install -y git rsync openssh-client
```

## Builds and rsync
In `services.toml`, each service can define build commands:
```toml
[services.build]
commands = [
    ["deno", "task", "convert-media"],
    ["deno", "task", "build"],
]
```

The build runs locally in the submodule before uploading the artifact with `rsync` by default. Use `sync_source` to specify the build output directory (e.g. `build`, `dist`)
