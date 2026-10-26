# Installing Ursine

Ursine ships as a release tarball containing both binaries, the PAM service and
init units for systemd, OpenRC and dinit. The installer is the same on every
Linux distribution; only paths and the init system are detected or selected.

## Nix profile

```bash
# 1. Add the flake
nix registry add ursine github:skea999/ursine

# 2. Install in the default profile
nix profile add ursine#ursine

# 3. Bootstrap the service (detects systemd, OpenRC or dinit)
sudo "$HOME/.nix-profile/bin/ursine-activate"
```

`ursine-activate` discovers the Nix profile generically, writes
`/etc/pam.d/ursine` as a symlink into that profile, renders the init unit with
the profile's `bin/ursined`, and restarts the service. Re-run it after every
upgrade:

```bash
nix profile upgrade ursine
sudo "$HOME/.nix-profile/bin/ursine-activate"
```

The package chain is `ursine` → `ursined` → `ursine-activate`: the daemon owns
the activation tools and the GUI depends on the daemon. The activation command
is idempotent and refreshes changed PAM/unit assets.

Use `--init`, `--no-start`, or `--root <dir>` for advanced/staged tests.

To remove the service and PAM/init files:

```bash
sudo "$HOME/.nix-profile/bin/ursine-deactivate"
```

Add `--purge` to remove `/etc/ursine` and logs.

## Release tarball

```bash
mkdir -p ursine
tar xzf ursine-vX.Y.Z-x86_64-musl.tar.gz -C ursine
cd ursine
sudo ./install.sh
```

The release binaries are dynamic **musl** and target Alpine/FeralOS. On a
glibc distribution the installer can stage or package the files, but the
binaries need a musl runtime/loader or a distribution-specific build; glibc
artifacts are a v6 concern.

The installer:

- installs `ursine` (GUI) and `ursined` (root daemon);
- installs `/etc/pam.d/ursine` (auth + account stacks, `pam_unix.so`);
- installs the unit for the detected init system;
- creates `/etc/ursine` and `/var/log/ursine` (mode `0750`);
- enables and starts the daemon unless `--no-start` is passed.

Use `--check` first to print the resolved paths, PAM module and unit without
changing anything:

```bash
./install.sh --check
```

The installer executes `ursined --version` and refuses a stub-auth build unless
`--allow-stub` is passed. Stub auth accepts any non-empty password; use it only
for development. Staged installs try the same check; when the target cannot run
on the build host, cross-architecture staging fails unless
`--allow-unverified-auth` is passed; that flag warns and leaves the auth backend
unverified. CI runs the staged binary and asserts `auth: pam`.

On a build host without `pam_unix.so`, add `--skip-pam-check` (staging already
skips the target PAM check).

## Paths

Defaults follow the FHS. Every path can be overridden by flag or environment
variable; this is what makes the same installer usable by package builds on
different distributions.

| Flag | Environment | Default | Purpose |
|------|-------------|---------|---------|
| `--prefix` | `PREFIX` | `/usr/local` | Prefix for binaries and libexec |
| `--bindir` | `BINDIR` | `PREFIX/bin` | GUI binary |
| `--sbindir` | `SBINDIR` | `PREFIX/sbin` | Root daemon |
| `--libexecdir` | `LIBEXECDIR` | `PREFIX/lib` | dinit wrapper |
| `--sysconfdir` | `SYSCONFDIR` | `/etc` | Config root (`/etc/ursine/config.toml`) |
| `--pamdir` | `PAMDIR` | `/etc/pam.d` | PAM service directory |
| `--localstatedir` | `LOCALSTATEDIR` | `/var` | Audit/log directory |
| `--unitdir` | `UNITDIR` | init-dependent | systemd/OpenRC/dinit service directory |
| `--dinit-config-dir` | `DINIT_CONFIG_DIR` | `/etc/dinit.d/config` | dinit wrapper config |
| `--destdir` | `DESTDIR` | empty | Stage under a package root; no activation |
| `--allow-stub` | `ALLOW_STUB` | `0` | Permit a stub-auth daemon; development only |
| `--allow-unverified-auth` | `ALLOW_UNVERIFIED_AUTH` | `0` | Permit cross-architecture staging when the target binary cannot run |

Paths must be absolute and limited to `[A-Za-z0-9/._+-]`. Whitespace, control
characters, quotes, `$`, backticks and `%` are rejected because init systems do
not quote paths consistently and systemd treats `%` as a specifier.

## Init systems

The installer detects systemd, OpenRC and dinit from PID 1 and available
commands. PID 1 wins: installed OpenRC or dinit tooling alone does not select
the backend, so a FeralOS host carrying both packages stays on dinit. Force the
target when cross-building a package:

```bash
# systemd (most distributions)
sudo ./install.sh --init systemd

# OpenRC (Alpine, Gentoo)
sudo ./install.sh --init openrc

# dinit (FeralOS, Artix, Chimera); FeralOS uses /lib/dinit.d
sudo ./install.sh --init dinit --unitdir /lib/dinit.d --libexecdir /usr/lib
```

The dinit layout follows FeralOS conventions:

- service: `UNITDIR/ursined` (usually `/lib/dinit.d/ursined`)
- wrapper: `LIBEXECDIR/dinit/ursined`
- user config: `/etc/dinit.d/config/ursined.conf`

The service uses `smooth-recovery` and has no dependency on named boot
targets. Target names differ between dinit distributions (`local.target`,
`login.target`, `local`, `login`, `dbus`), and a missing target prevents the
service from loading. Enabled services start from `boot.d`; `ursined` binds its
socket without requiring D-Bus at startup.

## Package builds (staging)

`--destdir` installs into a package root and skips service activation. Pass
`--init` explicitly: auto-detection on the build host may not match the target.

```bash
# Debian/Fedora-style systemd package
./install.sh --destdir "$pkgdir" --prefix /usr --init systemd \
  --unitdir /usr/lib/systemd/system --no-start

# Alpine/OpenRC package
./install.sh --destdir "$pkgdir" --prefix /usr --init openrc \
  --unitdir /etc/init.d --no-start

# FeralOS dinit package
./install.sh --destdir "$pkgdir" --prefix /usr --init dinit \
  --unitdir /lib/dinit.d --libexecdir /usr/lib --no-start
```

Staging skips the `pam_unix.so` lookup because the target root is not the build
host. The PAM service file is still installed with mode `0644`. Run staging as
root or under `fakeroot` when the package must own the PAM file as `root:root`;
non-root staging emits a warning. The staged `ursined` is executed when the
build host can run it; cross-architecture staging requires
`--allow-unverified-auth`, and a stub-auth binary is refused unless
`--allow-stub` is explicit.

## PAM

The daemon calls `pam_start("ursine", ...)`, so Linux-PAM reads
`/etc/pam.d/ursine`. The file is:

```text
#%PAM-1.0
auth      required  pam_unix.so
account   required  pam_unix.so
```

No session is opened: Ursine verifies identity, it does not create a login
session. The file is installed `root:root 0644`.

The installer checks for `pam_unix.so` in the standard module directories. If
it is missing it stops with the package command for the detected distribution.
Use `--skip-pam-check` only when staging for another root.

An existing different `/etc/pam.d/ursine` is never overwritten silently: the
installer stops. `--force` creates a timestamped backup first.

## Nix

`nix profile add github:skea999/ursine` installs the binaries, but a Nix profile
cannot write `/etc/pam.d` or an init unit. On NixOS, configure PAM declaratively
with `security.pam.services.ursine`; on non-NixOS systems, run the release
installer (or a distro package) after installing the Nix package. New release
tarballs contain `install.sh` and `packaging/` at their root, so the same
installer can be run from the store path. The currently pinned v0.1.0 tarball
predates that layout; it is replaced on the next tagged release.

## Uninstall

The installer does not remove packages or user data. To remove a manual
install: stop and disable the unit, then remove the binaries, `/etc/pam.d/ursine`,
the unit file, `/etc/ursine`, and `/var/log/ursine`.
