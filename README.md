# Oxide

<p align="center">
  <img width="474" height="378" alt="logo" src="./oxide-logo.png" />
</p>

A fast, cache-first Node.js package manager written in Rust.

## Installation

### Windows

Download the latest `oxide-windows.exe` from [Releases](https://github.com/tjens23/oxide/releases/latest), and move it somewhere on your `PATH`:

```powershell
Move-Item .\oxide-latest.exe "$env:USERPROFILE\.local\bin\oxide.exe"
```

Or use `self-upgrade` to stay up to date after the first install:

```powershell
oxide self-upgrade
```

### macOS

Download the latest `oxide-macos` binary from [Releases](https://github.com/tjens23/oxide/releases/latest), make it executable, and move it to your `PATH`:

```sh
chmod +x oxide-macos
sudo mv oxide-macos /usr/local/bin/oxide
```

### Linux

Download the latest `oxide-linux` binary from [Releases](https://github.com/tjens23/oxide/releases/latest), make it executable, and move it to your `PATH`:

```sh
chmod +x oxide-linux
sudo mv oxide-linux /usr/local/bin/oxide
```

### Build from source

Requires [Rust](https://rustup.rs) 1.85+.

```sh
git clone https://github.com/tjens23/oxide
cd oxide
cargo build --release
# binary is at target/release/oxide
```

## Commands

| Command               | Description                                             |
| --------------------- | ------------------------------------------------------- |
| `install <package>`   | Install a package                                       |
| `uninstall <package>` | Uninstall a package                                     |
| `upgrade <package>`   | Upgrade a package                                       |
| `self-upgrade`        | Upgrade the oxide tool itself                           |
| `init`                | Initialize a new project with a package.json            |
| `run <script>`        | Run a defined package script                            |
| `exec <bin>`          | Execute a binary from node_modules/.bin                 |
| `dlx <package>`       | Fetch and run a package binary without installing it    |
| `link`                | Link a package globally or into node_modules            |
| `unlink`              | Remove a linked package                                 |
| `publish`             | Publish a package to the npm registry                   |
| `login`               | Authenticate with the npm registry                      |
| `outdated`            | List dependencies with available updates                |
| `upgrade`             | Upgrade installed packages                              |
| `why <package>`       | Explain why a package is installed                      |
| `ls`                  | List installed packages                                 |
| `pack`                | Create a publishable .tgz tarball locally               |
| `doctor`              | Check project health and environment                    |
| `workspaces`          | List workspace packages defined in this monorepo        |
| `foreach <script>`    | Run a script across all workspace packages              |
| `version`             | Bump package.json version, commit, and create a git tag |

## Usage

```sh
oxide install lodash
oxide install lodash@4.17.21
oxide uninstall lodash
oxide run build
oxide dlx create-react-app my-app
oxide outdated
oxide why lodash
```

### Workspaces

```sh
oxide install lodash --filter my-package
oxide foreach build
```

## Credits

<table>
  <tbody>
    <tr>
      <td align="center"><a href="https://github.com/MidnightRocket"><img src="https://github.com/MidnightRocket.png?size=100" width="100px;" alt="Tjens23"/><br /><sub><b>MidnightRocket</b></sub></a></td>
      <td align="center"><a href="https://github.com/conaticus"><img src="https://github.com/conaticus.png?size=100" width="100px;" alt="EnderSlain"/><br /><sub><b>Conaticus</b></sub></a></td>
    </tr>
  </tbody>
</table>

## Contributors

<table>
  <tbody>
    <tr>
      <td align="center"><a href="https://github.com/tjens23"><img src="https://github.com/tjens23.png?size=100" width="100px;" alt="Tjens23"/><br /><sub><b>Tjens23</b></sub></a></td>
      <td align="center"><a href="https://github.com/EnderSlain"><img src="https://github.com/EnderSlain.png?size=100" width="100px;" alt="EnderSlain"/><br /><sub><b>EnderSlain</b></sub></a></td>
    </tr>
  </tbody>
</table>
