# Install

Prebuilt binaries for macOS (Apple Silicon and Intel), Linux x86_64 and
Windows x86_64 are attached to each [GitHub release](https://github.com/edouard38000/synthetic-customers/releases),
with a `SHA256SUMS.txt` checksum file. The install scripts pick the right one,
check its checksum and install it for your user only (no sudo, no admin, no
token).

macOS or Linux (installs to `~/.local/bin`):

```bash
curl -fsSL https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.sh | sh
```

Windows, in PowerShell (installs to `%LOCALAPPDATA%\Programs\synthetic-customers\bin`):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.ps1 | iex"
```

To pick a release or a folder, set `SYNTHETIC_CUSTOMERS_VERSION` (e.g. `v0.1.0`)
or `SYNTHETIC_CUSTOMERS_INSTALL_DIR` before running the script. The scripts
print how to add the folder to your PATH if it isn't there yet.

From source, with Rust installed:

```bash
cargo install --git https://github.com/edouard38000/synthetic-customers
```

## Making a release (maintainers)

Bump `version` in `Cargo.toml`, merge, then push a matching tag:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

`.github/workflows/release.yml` builds the four binaries and publishes the
release. A tag with a hyphen (e.g. `v0.2.0-rc.1`) is marked as a pre-release.
