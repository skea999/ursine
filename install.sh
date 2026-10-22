#!/bin/sh
# Ursine installer — binaries, PAM service and init unit.
#
# Works on any Linux distribution with Linux-PAM. Paths are configurable; the
# defaults follow the FHS and the service layouts used by systemd, OpenRC and
# dinit. Packaging builds should pass --destdir and an explicit --init.
#
# Run as root for a real install. --destdir stages files without touching the
# host and skips service activation.
#
# Usage: ./install.sh [options]
#        ./install.sh --help

set -eu

# ── Defaults (overridable by environment or flags) ──────────────────────────
DESTDIR=${DESTDIR:-}
PREFIX=${PREFIX:-/usr/local}
BINDIR=${BINDIR:-}
SBINDIR=${SBINDIR:-}
LIBEXECDIR=${LIBEXECDIR:-}
SYSCONFDIR=${SYSCONFDIR:-/etc}
PAMDIR=${PAMDIR:-/etc/pam.d}
LOCALSTATEDIR=${LOCALSTATEDIR:-/var}
UNITDIR=${UNITDIR:-}
DINIT_CONFIG_DIR=${DINIT_CONFIG_DIR:-}
BIN_SRC=${BIN_SRC:-}
INIT=${INIT:-auto}
FORCE=${FORCE:-0}
START=${START:-1}
ALLOW_STUB=${ALLOW_STUB:-0}
ALLOW_UNVERIFIED_AUTH=${ALLOW_UNVERIFIED_AUTH:-0}
INSTALL_PAM=${INSTALL_PAM:-1}
INSTALL_SERVICE=${INSTALL_SERVICE:-1}
PAM_CHECK=${PAM_CHECK:-1}
CHECK=0

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)

die() {
  printf 'install: error: %s\n' "$*" >&2
  exit 1
}

warn() {
  printf 'install: warning: %s\n' "$*" >&2
}

info() {
  printf 'install: %s\n' "$*"
}

usage() {
  cat <<'EOF'
Ursine installer — installs ursine, ursined, the PAM service and an init unit.

USAGE
  sudo ./install.sh [options]

OPTIONS
  --prefix DIR          Install prefix (default: /usr/local)
  --bindir DIR          GUI directory (default: PREFIX/bin)
  --sbindir DIR         Daemon directory (default: PREFIX/sbin)
  --libexecdir DIR      Internal helpers (default: PREFIX/lib)
  --sysconfdir DIR      Configuration root (default: /etc)
  --pamdir DIR          PAM service directory (default: /etc/pam.d)
  --localstatedir DIR   Variable data root (default: /var)
  --unitdir DIR         Init service directory (default depends on --init)
  --dinit-config-dir DIR
                        dinit user config directory (default: SYSCONFDIR/dinit.d/config)
  --destdir DIR         Stage everything under DIR; skip activation
  --bin-dir DIR         Directory containing the ursine/ursined binaries
  --init NAME           systemd, openrc, dinit or none (default: auto-detect)
  --no-pam              Do not install the PAM service
  --no-service          Do not install an init unit
  --no-start            Install the unit but do not enable/start it
  --skip-pam-check      Do not require pam_unix.so at install time
  --allow-stub          Allow installing a stub-auth (non-PAM) daemon; dev only
  --allow-unverified-auth
                        Permit cross-architecture staging when the target
                        binary cannot run; auth backend stays unverified
  --force               Overwrite changed PAM/unit files (backup first)
  --check               Print the resolved plan and exit
  --help                Show this help

ENVIRONMENT
  DESTDIR, PREFIX, BINDIR, SBINDIR, LIBEXECDIR, SYSCONFDIR, PAMDIR,
  LOCALSTATEDIR, UNITDIR, DINIT_CONFIG_DIR, BIN_SRC, INIT, START, ALLOW_STUB,
  ALLOW_UNVERIFIED_AUTH.

EXAMPLES
  sudo ./install.sh
  sudo ./install.sh --prefix /usr --init systemd
  ./install.sh --destdir "$pkgdir" --prefix /usr --init openrc --unitdir /etc/init.d
  ./install.sh --destdir "$pkgdir" --prefix /usr --init dinit \
      --unitdir /lib/dinit.d --libexecdir /usr/lib
EOF
}

# ── Argument parsing ────────────────────────────────────────────────────────
need_value() {
  [ "$#" -ge 2 ] || die "option $1 needs a value"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
  --prefix)
    need_value "$@"
    PREFIX=$2
    shift 2
    ;;
  --prefix=*)
    PREFIX=${1#*=}
    shift
    ;;
  --bindir)
    need_value "$@"
    BINDIR=$2
    shift 2
    ;;
  --bindir=*)
    BINDIR=${1#*=}
    shift
    ;;
  --sbindir)
    need_value "$@"
    SBINDIR=$2
    shift 2
    ;;
  --sbindir=*)
    SBINDIR=${1#*=}
    shift
    ;;
  --libexecdir)
    need_value "$@"
    LIBEXECDIR=$2
    shift 2
    ;;
  --libexecdir=*)
    LIBEXECDIR=${1#*=}
    shift
    ;;
  --sysconfdir)
    need_value "$@"
    SYSCONFDIR=$2
    shift 2
    ;;
  --sysconfdir=*)
    SYSCONFDIR=${1#*=}
    shift
    ;;
  --pamdir)
    need_value "$@"
    PAMDIR=$2
    shift 2
    ;;
  --pamdir=*)
    PAMDIR=${1#*=}
    shift
    ;;
  --localstatedir)
    need_value "$@"
    LOCALSTATEDIR=$2
    shift 2
    ;;
  --localstatedir=*)
    LOCALSTATEDIR=${1#*=}
    shift
    ;;
  --unitdir)
    need_value "$@"
    UNITDIR=$2
    shift 2
    ;;
  --unitdir=*)
    UNITDIR=${1#*=}
    shift
    ;;
  --dinit-config-dir)
    need_value "$@"
    DINIT_CONFIG_DIR=$2
    shift 2
    ;;
  --dinit-config-dir=*)
    DINIT_CONFIG_DIR=${1#*=}
    shift
    ;;
  --destdir)
    need_value "$@"
    DESTDIR=$2
    shift 2
    ;;
  --destdir=*)
    DESTDIR=${1#*=}
    shift
    ;;
  --bin-dir | --from)
    need_value "$@"
    BIN_SRC=$2
    shift 2
    ;;
  --bin-dir=* | --from=*)
    BIN_SRC=${1#*=}
    shift
    ;;
  --init)
    need_value "$@"
    INIT=$2
    shift 2
    ;;
  --init=*)
    INIT=${1#*=}
    shift
    ;;
  --no-pam)
    INSTALL_PAM=0
    shift
    ;;
  --no-service)
    INSTALL_SERVICE=0
    shift
    ;;
  --no-start)
    START=0
    shift
    ;;
  --skip-pam-check)
    PAM_CHECK=0
    shift
    ;;
  --allow-stub)
    ALLOW_STUB=1
    shift
    ;;
  --allow-unverified-auth)
    ALLOW_UNVERIFIED_AUTH=1
    shift
    ;;
  --force)
    FORCE=1
    shift
    ;;
  --check)
    CHECK=1
    shift
    ;;
  --help | -h)
    usage
    exit 0
    ;;
  *) die "unknown option: $1 (try --help)" ;;
  esac
done

# ── Paths ───────────────────────────────────────────────────────────────────
trim_trailing_slash() {
  if [ "$1" = "/" ]; then
    printf '/'
  else
    printf '%s' "${1%/}"
  fi
}

PREFIX=$(trim_trailing_slash "$PREFIX")
SYSCONFDIR=$(trim_trailing_slash "$SYSCONFDIR")
PAMDIR=$(trim_trailing_slash "$PAMDIR")
LOCALSTATEDIR=$(trim_trailing_slash "$LOCALSTATEDIR")
[ -n "$DESTDIR" ] && DESTDIR=$(trim_trailing_slash "$DESTDIR")
[ -n "$BINDIR" ] || BINDIR="$PREFIX/bin"
[ -n "$SBINDIR" ] || SBINDIR="$PREFIX/sbin"
[ -n "$LIBEXECDIR" ] || LIBEXECDIR="$PREFIX/lib"
[ -n "$DINIT_CONFIG_DIR" ] || DINIT_CONFIG_DIR="$SYSCONFDIR/dinit.d/config"

validate_path() {
  [ -n "$1" ] || die "empty path is not allowed"
  case "$1" in
  /*) ;;
  *) die "path must be absolute: $1" ;;
  esac
  case "$1" in
  *' '* | *'	'*) die "path must not contain whitespace: $1" ;;
  esac
  case "$1" in
  *[!A-Za-z0-9/._+-]*) die "path contains unsafe characters: $1" ;;
  esac
  if [ "$1" != "/" ]; then
    case "$1/" in
    *//* | */../* | */./*) die "path contains empty, . or .. component: $1" ;;
    esac
  fi
}

for path in "$PREFIX" "$BINDIR" "$SBINDIR" "$LIBEXECDIR" "$SYSCONFDIR" \
  "$PAMDIR" "$LOCALSTATEDIR" "$DINIT_CONFIG_DIR"; do
  validate_path "$path"
done
[ -z "$UNITDIR" ] || validate_path "$UNITDIR"

case "$DESTDIR" in
'') ;;
/*) validate_path "$DESTDIR" ;;
*) die "--destdir must be absolute: $DESTDIR" ;;
esac

target_path() {
  printf '%s%s' "$DESTDIR" "$1"
}

# ── Assets and binaries ─────────────────────────────────────────────────────
find_asset_dir() {
  for dir in "$SCRIPT_DIR/packaging" "$SCRIPT_DIR/share/ursine/packaging" \
    "$SCRIPT_DIR/../packaging"; do
    if [ -d "$dir/pam.d" ]; then
      printf '%s' "$dir"
      return 0
    fi
  done
  return 1
}

find_bin_dir() {
  for dir in "$SCRIPT_DIR/bin" "$SCRIPT_DIR/artifacts/target/release" \
    "$SCRIPT_DIR/../artifacts/target/release"; do
    if [ -f "$dir/ursine" ] && [ -f "$dir/ursined" ]; then
      printf '%s' "$dir"
      return 0
    fi
  done
  return 1
}

if [ -z "$BIN_SRC" ]; then
  BIN_SRC=$(find_bin_dir) || die "binaries not found (use --bin-dir DIR)"
fi
BIN_SRC=$(CDPATH='' cd -- "$BIN_SRC" && pwd -P) || die "cannot enter --bin-dir: $BIN_SRC"
[ -f "$BIN_SRC/ursine" ] || die "missing $BIN_SRC/ursine"
[ -f "$BIN_SRC/ursined" ] || die "missing $BIN_SRC/ursined"

# Refuse to install a dev stub authenticator as a root service by accident.
# Execute the target binary when possible, including DESTDIR staging. Cross-
# architecture staging cannot execute the target and must opt in explicitly.
if [ "$CHECK" -eq 0 ]; then
  if version=$("$BIN_SRC/ursined" --version 2>/dev/null); then
    case "$version" in
    *"auth: pam"*) ;;
    *"auth: stub"*)
      if [ "$ALLOW_STUB" -ne 1 ]; then
        die "refusing stub-auth daemon; use a --features pam build (or --allow-stub for dev)"
      fi
      warn "installing STUB-AUTH daemon: any non-empty password is accepted"
      ;;
    *) die "unexpected ursined --version output: $version" ;;
    esac
  elif [ -n "$DESTDIR" ] && [ "$ALLOW_UNVERIFIED_AUTH" -eq 1 ]; then
    warn "cannot execute $BIN_SRC/ursined; staged auth backend is unverified"
  elif [ -n "$DESTDIR" ]; then
    die "cannot execute $BIN_SRC/ursined; use --allow-unverified-auth for cross-architecture staging"
  else
    die "cannot execute $BIN_SRC/ursined --version (wrong architecture or missing musl runtime?)"
  fi
fi

if [ "$INSTALL_PAM" -eq 1 ] || { [ "$INSTALL_SERVICE" -eq 1 ] && [ "$INIT" != none ]; }; then
  ASSET_DIR=$(find_asset_dir) || die "packaging assets not found (run from the release tree)"
fi

# ── OS and init detection ───────────────────────────────────────────────────
os_value() {
  [ -r /etc/os-release ] || return 1
  sed -n "s/^$1=//p" /etc/os-release 2>/dev/null | head -n 1 | tr -d '"'
}

OS_ID=$(os_value ID || true)
OS_LIKE=$(os_value ID_LIKE || true)
OS_NAME=$(os_value PRETTY_NAME || true)
OS_ID=$(printf '%s' "$OS_ID" | tr '[:upper:]' '[:lower:]')
OS_LIKE=$(printf '%s' "$OS_LIKE" | tr '[:upper:]' '[:lower:]')

detect_init() {
  pid1=$(ps -p 1 -o comm= 2>/dev/null || true)
  case "$pid1" in
  systemd)
    printf systemd
    return 0
    ;;
  dinit)
    printf dinit
    return 0
    ;;
  init | openrc-init)
    if command -v rc-service >/dev/null 2>&1; then
      printf openrc
      return 0
    fi
    ;;
  esac
  if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
    printf systemd
  elif command -v dinitctl >/dev/null 2>&1 &&
    { [ -d /etc/dinit.d ] || [ -d /lib/dinit.d ]; }; then
    printf dinit
  elif command -v rc-service >/dev/null 2>&1 && [ -d /etc/init.d ]; then
    printf openrc
  else
    printf none
  fi
}

if [ "$INIT" = auto ]; then
  INIT=$(detect_init)
fi
case "$INIT" in
systemd | openrc | dinit | none) ;;
*) die "unknown --init value: $INIT (systemd, openrc, dinit or none)" ;;
esac

if [ -z "$UNITDIR" ]; then
  case "$INIT" in
  systemd) UNITDIR=/etc/systemd/system ;;
  openrc) UNITDIR=/etc/init.d ;;
  dinit) UNITDIR=/etc/dinit.d ;;
  none) UNITDIR= ;;
  esac
fi

if [ "$INSTALL_SERVICE" -eq 1 ] && [ "$INIT" = none ]; then
  warn "no supported init system detected; skipping unit (use --init or --no-service)"
  INSTALL_SERVICE=0
  START=0
fi

# ── Preflight ───────────────────────────────────────────────────────────────
if [ "$CHECK" -eq 0 ] && [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
  die "run as root, or use --destdir DIR to stage files"
fi

pam_module=
if [ "$INSTALL_PAM" -eq 1 ]; then
  [ -f "$ASSET_DIR/pam.d/ursine" ] || die "missing $ASSET_DIR/pam.d/ursine"
  if [ -z "$DESTDIR" ] && [ "$PAM_CHECK" -eq 1 ]; then
    for dir in /lib/security /lib64/security /usr/lib/security \
      /usr/lib64/security /lib/x86_64-linux-gnu/security \
      /usr/lib/x86_64-linux-gnu/security \
      /lib/aarch64-linux-gnu/security \
      /usr/lib/aarch64-linux-gnu/security; do
      if [ -f "$dir/pam_unix.so" ]; then
        pam_module="$dir/pam_unix.so"
        break
      fi
    done
    if [ -z "$pam_module" ]; then
      case "$OS_ID $OS_LIKE" in
      *alpine*) hint="apk add linux-pam" ;;
      *debian* | *ubuntu*) hint="apt-get install libpam-modules" ;;
      *fedora* | *rhel* | *centos*) hint="dnf install pam" ;;
      *arch*) hint="pacman -S pam" ;;
      *suse*) hint="zypper install pam" ;;
      *) hint="install Linux-PAM (pam_unix.so)" ;;
      esac
      die "pam_unix.so not found; run: $hint (or use --skip-pam-check)"
    fi
  fi
fi

if [ "$INSTALL_SERVICE" -eq 1 ]; then
  case "$INIT" in
  systemd) [ -f "$ASSET_DIR/systemd/ursined.service" ] || die "missing systemd unit asset" ;;
  openrc) [ -f "$ASSET_DIR/openrc/ursined" ] || die "missing OpenRC service asset" ;;
  dinit)
    [ -f "$ASSET_DIR/dinit/ursined" ] || die "missing dinit service asset"
    [ -f "$ASSET_DIR/dinit/ursined.wrapper" ] || die "missing dinit wrapper asset"
    [ -f "$ASSET_DIR/dinit/ursined.conf" ] || die "missing dinit config asset"
    ;;
  esac
fi

# ── Plan output ─────────────────────────────────────────────────────────────
show_plan() {
  info "distro: ${OS_NAME:-unknown} (${OS_ID:-unknown})"
  info "init: $INIT"
  info "prefix: $PREFIX"
  info "binaries: $BIN_SRC -> $BINDIR, $SBINDIR"
  info "config: $SYSCONFDIR/ursine/config.toml"
  if [ "$INSTALL_PAM" -eq 1 ]; then
    if [ -n "$DESTDIR" ]; then
      info "PAM service: $PAMDIR/ursine (target check skipped for staging)"
    elif [ -n "$pam_module" ]; then
      info "PAM service: $PAMDIR/ursine (module: $pam_module)"
    else
      info "PAM service: $PAMDIR/ursine (module check skipped)"
    fi
  fi
  if [ "$INSTALL_SERVICE" -eq 1 ]; then
    case "$INIT" in
    systemd) info "unit: $UNITDIR/ursined.service" ;;
    openrc) info "unit: $UNITDIR/ursined" ;;
    dinit)
      info "unit: $UNITDIR/ursined"
      info "dinit wrapper: $LIBEXECDIR/dinit/ursined"
      info "dinit config: $DINIT_CONFIG_DIR/ursined.conf"
      ;;
    esac
  else
    info "unit: not installed"
  fi
  info "audit directory: $LOCALSTATEDIR/log/ursine"
  if [ -n "$DESTDIR" ]; then
    info "mode: staging under $DESTDIR (no activation)"
  elif [ "$INSTALL_SERVICE" -eq 1 ] && [ "$START" -eq 1 ]; then
    info "mode: install and start"
  else
    info "mode: install only"
  fi
}

show_plan
[ "$CHECK" -eq 0 ] || exit 0

# ── Install helpers ─────────────────────────────────────────────────────────
ensure_dir() {
  install -d -m "$2" "$1" || die "cannot create $1"
}

# Escape a value for use in a sed replacement with | as the delimiter.
sed_escape() {
  printf '%s' "$1" | sed 's/[\\&|]/\\&/g'
}

render_to() {
  render_src=$1
  render_dst=$2
  sed \
    -e "s|@PREFIX@|$(sed_escape "$PREFIX")|g" \
    -e "s|@BINDIR@|$(sed_escape "$BINDIR")|g" \
    -e "s|@SBINDIR@|$(sed_escape "$SBINDIR")|g" \
    -e "s|@LIBEXECDIR@|$(sed_escape "$LIBEXECDIR")|g" \
    -e "s|@SYSCONFDIR@|$(sed_escape "$SYSCONFDIR")|g" \
    -e "s|@PAMDIR@|$(sed_escape "$PAMDIR")|g" \
    -e "s|@LOCALSTATEDIR@|$(sed_escape "$LOCALSTATEDIR")|g" \
    -e "s|@DINIT_CONFIG_DIR@|$(sed_escape "$DINIT_CONFIG_DIR")|g" \
    "$render_src" >"$render_dst" || return 1
}

backup_file() {
  backup="$1.ursine-backup.$(date +%Y%m%d%H%M%S)"
  cp -p "$1" "$backup" || die "cannot back up $1"
  info "backup: $backup"
}

install_file() {
  file_src=$1
  file_dst=$2
  file_mode=$3
  file_tmp=$(mktemp "${file_dst}.ursine-new.XXXXXX") || {
    die "cannot create temporary file next to $file_dst"
  }
  install -m "$file_mode" "$file_src" "$file_tmp" || {
    rm -f "$file_tmp"
    die "cannot install $file_dst"
  }
  mv -f "$file_tmp" "$file_dst" || {
    rm -f "$file_tmp"
    die "cannot replace $file_dst"
  }
}

install_rendered() {
  inst_src=$1
  inst_dst=$2
  inst_mode=$3
  inst_tmp=$(mktemp "${inst_dst}.ursine-tmp.XXXXXX") || {
    die "cannot create temporary file next to $inst_dst"
  }
  render_to "$inst_src" "$inst_tmp" || {
    rm -f "$inst_tmp"
    die "cannot render $inst_src"
  }
  if [ -L "$inst_dst" ]; then
    rm -f "$inst_tmp"
    die "refusing to replace symlink: $inst_dst"
  fi
  if [ -d "$inst_dst" ]; then
    rm -f "$inst_tmp"
    die "refusing to replace directory: $inst_dst"
  fi
  if [ -e "$inst_dst" ]; then
    if cmp -s "$inst_tmp" "$inst_dst"; then
      rm -f "$inst_tmp"
      chmod "$inst_mode" "$inst_dst" || die "cannot set mode on $inst_dst"
      info "unchanged: $inst_dst"
      return 0
    fi
    if [ "$FORCE" -ne 1 ]; then
      rm -f "$inst_tmp"
      die "refusing to overwrite $inst_dst (use --force)"
    fi
    backup_file "$inst_dst"
  fi
  install -m "$inst_mode" "$inst_tmp" "$inst_dst" || {
    rm -f "$inst_tmp"
    die "cannot install $inst_dst"
  }
  rm -f "$inst_tmp"
  info "installed: $inst_dst"
}

# ── Install ─────────────────────────────────────────────────────────────────
LOG_DIR="$LOCALSTATEDIR/log/ursine"

ensure_dir "$(target_path "$BINDIR")" 0755
ensure_dir "$(target_path "$SBINDIR")" 0755
ensure_dir "$(target_path "$SYSCONFDIR/ursine")" 0750
ensure_dir "$(target_path "$LOG_DIR")" 0750

if [ "$INSTALL_PAM" -eq 1 ]; then
  if [ -z "$DESTDIR" ]; then
    [ -d "$PAMDIR" ] || die "PAM directory not found: $PAMDIR"
  fi
  ensure_dir "$(target_path "$PAMDIR")" 0755
  PAM_TARGET=$(target_path "$PAMDIR/ursine")
  if [ -L "$PAM_TARGET" ]; then
    die "refusing to replace symlink: $PAM_TARGET"
  fi
  if [ -d "$PAM_TARGET" ]; then
    die "refusing to replace directory: $PAM_TARGET"
  fi
  if [ -n "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
    warn "staging PAM as non-root: ownership will be the builder; use root or fakeroot"
  fi
  if [ -e "$PAM_TARGET" ] && ! cmp -s "$ASSET_DIR/pam.d/ursine" "$PAM_TARGET"; then
    [ "$FORCE" -eq 1 ] || die "refusing to overwrite $PAM_TARGET (use --force)"
    backup_file "$PAM_TARGET"
  fi
  install_file "$ASSET_DIR/pam.d/ursine" "$PAM_TARGET" 0644
  info "installed: $PAM_TARGET"
fi

if [ "$INSTALL_SERVICE" -eq 1 ]; then
  case "$INIT" in
  systemd)
    ensure_dir "$(target_path "$UNITDIR")" 0755
    install_rendered "$ASSET_DIR/systemd/ursined.service" \
      "$(target_path "$UNITDIR/ursined.service")" 0644
    ;;
  openrc)
    ensure_dir "$(target_path "$UNITDIR")" 0755
    install_rendered "$ASSET_DIR/openrc/ursined" \
      "$(target_path "$UNITDIR/ursined")" 0755
    ;;
  dinit)
    ensure_dir "$(target_path "$UNITDIR")" 0755
    ensure_dir "$(target_path "$LIBEXECDIR/dinit")" 0755
    ensure_dir "$(target_path "$DINIT_CONFIG_DIR")" 0755
    install_rendered "$ASSET_DIR/dinit/ursined" \
      "$(target_path "$UNITDIR/ursined")" 0644
    install_rendered "$ASSET_DIR/dinit/ursined.wrapper" \
      "$(target_path "$LIBEXECDIR/dinit/ursined")" 0755
    install_rendered "$ASSET_DIR/dinit/ursined.conf" \
      "$(target_path "$DINIT_CONFIG_DIR/ursined.conf")" 0644
    ;;
  esac
fi

install_file "$BIN_SRC/ursine" "$(target_path "$BINDIR/ursine")" 0755
info "installed: $(target_path "$BINDIR/ursine")"
install_file "$BIN_SRC/ursined" "$(target_path "$SBINDIR/ursined")" 0755
info "installed: $(target_path "$SBINDIR/ursined")"

if [ -z "$DESTDIR" ]; then
  chown root:root "$(target_path "$LOG_DIR")" || die "cannot set ownership on $LOG_DIR"
fi

if [ -z "$DESTDIR" ] && [ "$INSTALL_SERVICE" -eq 1 ] && [ "$START" -eq 1 ]; then
  case "$INIT" in
  systemd)
    systemctl daemon-reload || die "systemctl daemon-reload failed"
    systemctl enable --now ursined.service || die "systemctl enable --now failed"
    info "started: ursined.service"
    ;;
  openrc)
    rc-update add ursined default || die "rc-update add failed"
    rc-service ursined start || die "rc-service start failed"
    info "started: ursined"
    ;;
  dinit)
    dinitctl enable ursined || warn "could not enable ursined; run: dinitctl enable ursined"
    dinitctl start ursined || die "dinitctl start failed"
    info "started: ursined"
    ;;
  esac
else
  case "$INIT" in
  systemd)
    [ "$INSTALL_SERVICE" -eq 1 ] && info "enable manually: systemctl enable --now ursined.service"
    ;;
  openrc)
    [ "$INSTALL_SERVICE" -eq 1 ] && info "enable manually: rc-update add ursined default && rc-service ursined start"
    ;;
  dinit)
    [ "$INSTALL_SERVICE" -eq 1 ] && info "enable manually: dinitctl enable ursined && dinitctl start ursined"
    ;;
  esac
fi

info "done"
