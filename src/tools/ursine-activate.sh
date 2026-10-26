#!/bin/sh
# Ursine system activation for Nix profile installs.
#
# Run as root after installing or upgrading the Nix profile:
#   nix profile add --profile /nix/var/nix/profiles/ursine \
#     github:skea999/ursine#ursine
#   sudo /nix/var/nix/profiles/ursine/bin/ursine-activate
#
# The script is idempotent. Re-run it after every upgrade so changed PAM or
# init-unit assets are applied. The daemon and PAM policy are copied to
# root-owned paths: the Nix profile is user-writable and must never be able to
# replace the root service after activation.
set -eu

PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/run/current-system/sw/bin
export PATH

PROFILE=${URSINE_PROFILE:-}
if [ -z "$PROFILE" ]; then
  # Generic profile discovery: never hardcode a user or home directory.
  # sudo sets SUDO_HOME; getent handles environments where it is absent.
  if [ -n "${SUDO_HOME:-}" ] && [ -x "$SUDO_HOME/.nix-profile/bin/ursined" ]; then
    PROFILE="$SUDO_HOME/.nix-profile"
  elif [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
    user_home=$(getent passwd "$SUDO_USER" 2>/dev/null | cut -d: -f6) || user_home=
    [ -n "$user_home" ] || user_home="/home/$SUDO_USER"
    if [ -x "$user_home/.nix-profile/bin/ursined" ]; then
      PROFILE="$user_home/.nix-profile"
    fi
  fi
  if [ -z "$PROFILE" ] && [ -x "${HOME:-}/.nix-profile/bin/ursined" ]; then
    PROFILE="${HOME}/.nix-profile"
  fi
  [ -n "$PROFILE" ] || PROFILE=/nix/var/nix/profiles/default
fi
ROOT=
INIT=
START=1
FORCE=0
DRY_RUN=0
CHANGED=0
ALLOW_STUB=0

usage() {
  cat <<'EOF'
Usage: ursine-activate [options]

Install or refresh the Ursine PAM service and init unit from a Nix profile.

Options:
  --profile PATH   Nix profile containing bin/ursined and packaging/
                   (default: /nix/var/nix/profiles/ursine)
  --init NAME      force systemd, openrc, or dinit (default: auto-detect)
  --root PATH      stage files under PATH instead of /; implies --no-start
  --no-start       install files but do not enable/restart the service
  --force          replace an existing different /etc/pam.d/ursine
  --allow-stub     allow a stub-auth daemon (development only)
  --dry-run        print actions without writing
  -h, --help       show this help
EOF
}

die() {
  printf 'ursine-activate: %s\n' "$*" >&2
  exit 1
}

log() {
  printf 'ursine-activate: %s\n' "$*"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
  --profile)
    [ "$#" -ge 2 ] || die "--profile needs a path"
    PROFILE=$2
    shift 2
    ;;
  --init)
    [ "$#" -ge 2 ] || die "--init needs a value"
    INIT=$2
    shift 2
    ;;
  --root)
    [ "$#" -ge 2 ] || die "--root needs a path"
    [ -n "$2" ] || die "--root needs a non-empty path"
    ROOT=$2
    shift 2
    ;;
  --no-start)
    START=0
    shift
    ;;
  --force)
    FORCE=1
    shift
    ;;
  --allow-stub)
    ALLOW_STUB=1
    shift
    ;;
  --dry-run)
    DRY_RUN=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    die "unknown argument: $1"
    ;;
  esac
done

[ -n "$PROFILE" ] || die "empty profile path"
case "$PROFILE" in
/*) ;;
*) PROFILE="$PWD/$PROFILE" ;;
esac
case "$PROFILE" in
*'
'*) die "profile path must not contain a newline" ;;
esac
[ -x "$PROFILE/bin/ursined" ] || die "missing executable: $PROFILE/bin/ursined"
[ -f "$PROFILE/packaging/pam.d/ursine" ] || die "missing PAM asset: $PROFILE/packaging/pam.d/ursine"
if [ "$DRY_RUN" -eq 0 ]; then
  version=$("$PROFILE/bin/ursined" --version 2>/dev/null) ||
    die "cannot execute $PROFILE/bin/ursined --version"
  case "$version" in
  *"auth: pam"*) ;;
  *"auth: stub"*)
    [ "$ALLOW_STUB" -eq 1 ] ||
      die "refusing stub-auth daemon; install a PAM build (or --allow-stub for dev)"
    log "warning: installing stub-auth daemon; any non-empty password is accepted"
    ;;
  *) die "unexpected ursined --version output: $version" ;;
  esac
fi

if [ -n "$ROOT" ]; then
  case "$ROOT" in
  /) die "--root must be a staging directory, not /" ;;
  esac
  if [ "$DRY_RUN" -eq 1 ]; then
    case "$ROOT" in
    /*) ;;
    *) ROOT="$PWD/$ROOT" ;;
    esac
  else
    mkdir -p "$ROOT"
    ROOT=$(CDPATH='' cd -- "$ROOT" && pwd -P)
  fi
  ROOT=${ROOT%/}
  [ -n "$ROOT" ] && [ "$ROOT" != / ] ||
    die "--root must be a staging directory, not /"
  case "$ROOT" in
  *..*) die "--root must not contain .." ;;
  *[!A-Za-z0-9/._+-]*) die "--root contains unsupported characters" ;;
  esac
  START=0
elif [ "$(id -u)" -ne 0 ]; then
  die "must run as root (use --root for staged tests)"
fi

[ "$DRY_RUN" -eq 1 ] && START=0

detect_init() {
  # PID 1 is authoritative. Presence of rc-service or dinitctl only means the
  # tooling is installed: FeralOS can carry both OpenRC and dinit packages.
  pid1=${URSINE_PID1_COMM:-}
  if [ -z "$pid1" ] && [ -r /proc/1/comm ]; then
    pid1=$(tr -d '\n' </proc/1/comm)
  fi
  [ -n "$pid1" ] || pid1=$(ps -p 1 -o comm= 2>/dev/null | tr -d '[:space:]' || true)
  if [ "$pid1" = init ]; then
    pid1_exe=${URSINE_PID1_EXE:-}
    if [ -z "$pid1_exe" ] && [ -L /proc/1/exe ]; then
      pid1_exe=$(readlink /proc/1/exe 2>/dev/null || true)
    fi
    case "$pid1_exe" in
    */dinit) pid1=dinit ;;
    */openrc-init) pid1=openrc-init ;;
    esac
  fi
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
    if [ -d /run/openrc ]; then
      printf openrc
      return 0
    fi
    if command -v dinitctl >/dev/null 2>&1 && dinitctl list >/dev/null 2>&1; then
      printf dinit
      return 0
    fi
    if command -v rc-service >/dev/null 2>&1; then
      printf openrc
      return 0
    fi
    ;;
  esac
  if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
    printf systemd
  elif command -v dinitctl >/dev/null 2>&1 && dinitctl list >/dev/null 2>&1; then
    printf dinit
  elif [ -d /run/openrc ] && command -v rc-service >/dev/null 2>&1; then
    printf openrc
  elif command -v rc-service >/dev/null 2>&1; then
    printf openrc
  elif command -v dinitctl >/dev/null 2>&1; then
    printf dinit
  else
    printf none
  fi
}

if [ -z "$INIT" ]; then
  INIT=$(detect_init)
  [ "$INIT" != none ] || die "cannot detect init; use --init systemd|openrc|dinit"
fi

case "$INIT" in
systemd | openrc | dinit) ;;
*) die "unsupported init: $INIT" ;;
esac

ETC="${ROOT}/etc"
VAR="${ROOT}/var"
LIBEXEC="${ROOT}/usr/lib"
BIN_DIR="${ROOT}/usr/local/sbin"
RUN_BIN="$BIN_DIR/ursined"
PAM_LINK="$ETC/pam.d/ursine"
PAM_SOURCE="$PROFILE/packaging/pam.d/ursine"
SYSTEMD_UNIT="$ETC/systemd/system/ursined.service"
OPENRC_INIT="$ETC/init.d/ursined"
DINIT_UNIT_DIR=${URSINE_DINIT_UNIT_DIR:-}
DINIT_UNIT_DIR=${DINIT_UNIT_DIR%/}
if [ -n "$DINIT_UNIT_DIR" ]; then
  case "$DINIT_UNIT_DIR" in
  /lib/dinit.d | /etc/dinit.d) ;;
  *) die "URSINE_DINIT_UNIT_DIR must be /lib/dinit.d or /etc/dinit.d" ;;
  esac
elif [ -d "${ROOT}/lib/dinit.d" ]; then
  # FeralOS uses /lib/dinit.d. Never let a stale /etc copy win.
  DINIT_UNIT_DIR=/lib/dinit.d
elif [ -d "${ROOT}/etc/dinit.d" ]; then
  DINIT_UNIT_DIR=/etc/dinit.d
else
  DINIT_UNIT_DIR=/lib/dinit.d
fi
DINIT_UNIT="${ROOT}${DINIT_UNIT_DIR}/ursined"
DINIT_WRAPPER="$LIBEXEC/dinit/ursined"
DINIT_CONFIG="$ETC/dinit.d/config/ursined.conf"

ensure_dir() {
  [ "$DRY_RUN" -eq 1 ] || mkdir -p "$1"
}

install_file() {
  src=$1
  dst=$2
  mode=$3
  ensure_dir "$(dirname "$dst")"
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would install $dst"
    return
  fi
  if [ -L "$dst" ]; then
    rm -f "$dst"
    CHANGED=1
  fi
  tmp=$(mktemp "${dst}.tmp.XXXXXX") || die "cannot create temp file for $dst"
  if ! install -m "$mode" "$src" "$tmp"; then
    rm -f "$tmp"
    die "cannot install $dst"
  fi
  if [ -f "$dst" ] && cmp -s "$tmp" "$dst"; then
    rm -f "$tmp"
    chmod "$mode" "$dst" || die "cannot set mode on $dst"
    if [ "$(id -u)" -eq 0 ]; then
      chown 0:0 "$dst" || die "cannot set owner on $dst"
    fi
  else
    mv -f "$tmp" "$dst"
    CHANGED=1
  fi
}

render_file() {
  src=$1
  dst=$2
  mode=$3
  ensure_dir "$(dirname "$dst")"
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would render $dst"
    return
  fi
  if [ -L "$dst" ]; then
    rm -f "$dst"
    CHANGED=1
  fi
  tmp=$(mktemp "${dst}.tmp.XXXXXX") || die "cannot create temp file for $dst"
  sed_escape() {
    printf '%s' "$1" | sed 's/[\\&|]/\\&/g'
  }
  sbindir=$(sed_escape "$BIN_DIR")
  sysconfdir=$(sed_escape "$ETC")
  localstatedir=$(sed_escape "$VAR")
  libexecdir=$(sed_escape "$LIBEXEC")
  dinit_config_dir=$(sed_escape "$ETC/dinit.d/config")
  if ! sed \
    -e "s|@SBINDIR@|$sbindir|g" \
    -e "s|@SYSCONFDIR@|$sysconfdir|g" \
    -e "s|@LOCALSTATEDIR@|$localstatedir|g" \
    -e "s|@LIBEXECDIR@|$libexecdir|g" \
    -e "s|@DINIT_CONFIG_DIR@|$dinit_config_dir|g" \
    "$src" >"$tmp"; then
    rm -f "$tmp"
    die "cannot render $dst"
  fi
  chmod "$mode" "$tmp"
  if [ -f "$dst" ] && cmp -s "$tmp" "$dst"; then
    rm -f "$tmp"
    chmod "$mode" "$dst" || die "cannot set mode on $dst"
    if [ "$(id -u)" -eq 0 ]; then
      chown 0:0 "$dst" || die "cannot set owner on $dst"
    fi
  else
    mv -f "$tmp" "$dst"
    CHANGED=1
  fi
}

remove_stale_unit() {
  path=$1
  if [ -L "$path" ]; then
    :
  elif [ -f "$path" ]; then
    grep -q 'Ursine' "$path" 2>/dev/null || return 0
  else
    return 0
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would remove stale $path"
  else
    rm -f "$path"
    CHANGED=1
  fi
}

remove_boot_link() {
  path=$1
  [ -L "$path" ] || return 0
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would remove stale $path"
  else
    rm -f "$path"
    CHANGED=1
  fi
}

remove_dinit_leftovers() {
  remove_stale_unit "${ROOT}/lib/dinit.d/ursined"
  remove_stale_unit "${ROOT}/etc/dinit.d/ursined"
  remove_boot_link "${ROOT}/lib/dinit.d/boot.d/ursined"
  remove_boot_link "${ROOT}/etc/dinit.d/boot.d/ursined"
}

install_pam() {
  ensure_dir "$(dirname "$PAM_LINK")"
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would install $PAM_LINK"
    return
  fi
  if [ -L "$PAM_LINK" ]; then
    # Migrate the old user-profile symlink to a root-owned copy.
    rm -f "$PAM_LINK"
    CHANGED=1
  elif [ -e "$PAM_LINK" ] && ! cmp -s "$PAM_SOURCE" "$PAM_LINK"; then
    if grep -q 'Ursine authentication service' "$PAM_LINK" 2>/dev/null; then
      backup="$PAM_LINK.ursine-backup.$(date +%Y%m%d%H%M%S)"
      cp -p "$PAM_LINK" "$backup" || die "cannot back up $PAM_LINK"
      log "backup: $backup"
    elif [ "$FORCE" -ne 1 ]; then
      die "$PAM_LINK exists and differs; use --force to replace it"
    fi
  fi
  install_file "$PAM_SOURCE" "$PAM_LINK" 0644
}

ensure_dir "$ETC"
ensure_dir "$ETC/ursine"
ensure_dir "$VAR/log/ursine"
if [ "$DRY_RUN" -eq 0 ]; then
  chmod 0750 "$ETC/ursine" "$VAR/log/ursine"
fi

install_pam
install_file "$PROFILE/bin/ursined" "$RUN_BIN" 0755

case "$INIT" in
systemd)
  remove_dinit_leftovers
  remove_stale_unit "$OPENRC_INIT"
  render_file "$PROFILE/packaging/systemd/ursined.service" "$SYSTEMD_UNIT" 0644
  ;;
openrc)
  remove_dinit_leftovers
  remove_stale_unit "$SYSTEMD_UNIT"
  render_file "$PROFILE/packaging/openrc/ursined" "$OPENRC_INIT" 0755
  ;;
dinit)
  remove_dinit_leftovers
  render_file "$PROFILE/packaging/dinit/ursined" "$DINIT_UNIT" 0644
  render_file "$PROFILE/packaging/dinit/ursined.wrapper" "$DINIT_WRAPPER" 0755
  if [ -e "$DINIT_CONFIG" ] || [ -L "$DINIT_CONFIG" ]; then
    log "keeping existing dinit config: $DINIT_CONFIG"
  else
    install_file "$PROFILE/packaging/dinit/ursined.conf" "$DINIT_CONFIG" 0644
  fi
  # Older activators could pick OpenRC just because rc-service existed. Remove
  # that stale Ursine unit so a later OpenRC command cannot manage it.
  remove_stale_unit "$OPENRC_INIT"
  remove_stale_unit "$SYSTEMD_UNIT"
  ;;
esac

if [ "$START" -eq 1 ]; then
  case "$INIT" in
  systemd)
    command -v systemctl >/dev/null 2>&1 || die "systemctl not found"
    systemctl daemon-reload
    systemctl enable ursined
    systemctl restart ursined
    ;;
  openrc)
    command -v rc-service >/dev/null 2>&1 || die "rc-service not found"
    command -v rc-update >/dev/null 2>&1 || die "rc-update not found"
    rc-update add ursined default
    rc-service ursined restart 2>/dev/null || rc-service ursined start
    ;;
  dinit)
    command -v dinitctl >/dev/null 2>&1 || die "dinitctl not found"
    # dinitctl returns non-zero when the service is already enabled. Do not
    # parse its English message: warn, then let start decide whether the
    # service is usable. This keeps upgrades idempotent across dinit versions.
    if ! enable_out=$(dinitctl enable ursined 2>&1); then
      if [ ! -L "${ROOT}/etc/dinit.d/boot.d/ursined" ] &&
        [ ! -L "${ROOT}/lib/dinit.d/boot.d/ursined" ]; then
        die "dinitctl enable ursined failed: $enable_out"
      fi
      log "dinitctl enable: already enabled ($enable_out)"
    fi
    # A running dinit keeps the old description cached. Reload it first so a
    # changed service file is used by the restart.
    dinitctl reload ursined >/dev/null 2>&1 || true
    dinitctl restart ursined 2>/dev/null || dinitctl start ursined ||
      die "dinitctl start ursined failed"
    ;;
  esac
fi

if [ "$DRY_RUN" -eq 1 ]; then
  log "dry-run complete for $INIT"
elif [ "$CHANGED" -eq 1 ]; then
  log "updated Ursine files for $INIT"
else
  log "Ursine files already current for $INIT"
fi
if [ "$START" -eq 1 ]; then
  log "ursined restarted"
else
  log "service not started"
fi
