#!/bin/sh
# Deactivate Ursine system integration installed by ursine-activate.
#
# This removes the PAM service and the selected init unit, disables/stops the
# service, and leaves /etc/ursine and /var/log/ursine in place unless --purge is
# passed.
set -eu

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
FORCE=0
PURGE=0
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: ursine-deactivate [options]

Remove the Ursine PAM service and init unit.

Options:
  --profile PATH   Nix profile used by the activation (default: inferred)
  --init NAME      force systemd, openrc, or dinit (default: auto-detect)
  --root PATH      stage removals under PATH instead of /
  --force          remove a non-symlink /etc/pam.d/ursine
  --purge          also remove /etc/ursine and /var/log/ursine
  --dry-run        print actions without removing
  -h, --help       show this help
EOF
}

die() {
  printf 'ursine-deactivate: %s\n' "$*" >&2
  exit 1
}

log() {
  printf 'ursine-deactivate: %s\n' "$*"
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
    ROOT=$2
    shift 2
    ;;
  --force)
    FORCE=1
    shift
    ;;
  --purge)
    PURGE=1
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

ROOT=${ROOT%/}
[ -n "$PROFILE" ] || die "empty profile path"
case "$PROFILE" in
/*) ;;
*) PROFILE="$PWD/$PROFILE" ;;
esac

if [ -n "$ROOT" ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    case "$ROOT" in
    /*) ;;
    *) ROOT="$PWD/$ROOT" ;;
    esac
  else
    mkdir -p "$ROOT"
    ROOT=$(CDPATH='' cd -- "$ROOT" && pwd)
  fi
elif [ "$(id -u)" -ne 0 ]; then
  die "must run as root (use --root for staged tests)"
fi

detect_init() {
  # PID 1 is authoritative. Presence of rc-service or dinitctl only means the
  # tooling is installed: FeralOS can carry both OpenRC and dinit packages.
  pid1=
  if [ -r /proc/1/comm ]; then
    pid1=$(tr -d '\n' </proc/1/comm)
  fi
  [ -n "$pid1" ] || pid1=$(ps -p 1 -o comm= 2>/dev/null | tr -d '[:space:]' || true)
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
    if [ -d /run/openrc ] || command -v rc-service >/dev/null 2>&1; then
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
PAM_LINK="$ETC/pam.d/ursine"
SYSTEMD_UNIT="$ETC/systemd/system/ursined.service"
OPENRC_INIT="$ETC/init.d/ursined"
DINIT_UNIT_LIB="${ROOT}/lib/dinit.d/ursined"
DINIT_UNIT_ETC="${ROOT}/etc/dinit.d/ursined"
DINIT_WRAPPER="$LIBEXEC/dinit/ursined"
DINIT_CONFIG="$ETC/dinit.d/config/ursined.conf"

remove_file() {
  path=$1
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would remove $path"
  elif [ -e "$path" ] || [ -L "$path" ]; then
    rm -f "$path"
    log "removed $path"
  fi
}

if [ "$DRY_RUN" -eq 0 ] && [ -z "$ROOT" ]; then
  case "$INIT" in
  systemd)
    command -v systemctl >/dev/null 2>&1 || die "systemctl not found"
    systemctl stop ursined 2>/dev/null || true
    systemctl disable ursined 2>/dev/null || true
    remove_file "$SYSTEMD_UNIT"
    systemctl daemon-reload
    ;;
  openrc)
    command -v rc-service >/dev/null 2>&1 || die "rc-service not found"
    command -v rc-update >/dev/null 2>&1 || die "rc-update not found"
    rc-service ursined stop 2>/dev/null || true
    rc-update del ursined default 2>/dev/null || true
    remove_file "$OPENRC_INIT"
    ;;
  dinit)
    command -v dinitctl >/dev/null 2>&1 || die "dinitctl not found"
    dinitctl stop ursined 2>/dev/null || true
    dinitctl disable ursined 2>/dev/null || true
    remove_file "$DINIT_UNIT_LIB"
    remove_file "$DINIT_UNIT_ETC"
    remove_file "$DINIT_WRAPPER"
    remove_file "$DINIT_CONFIG"
    ;;
  esac
else
  case "$INIT" in
  systemd) remove_file "$SYSTEMD_UNIT" ;;
  openrc) remove_file "$OPENRC_INIT" ;;
  dinit)
    remove_file "$DINIT_UNIT_LIB"
    remove_file "$DINIT_UNIT_ETC"
    remove_file "$DINIT_WRAPPER"
    remove_file "$DINIT_CONFIG"
    ;;
  esac
fi

if [ -L "$PAM_LINK" ]; then
  remove_file "$PAM_LINK"
elif [ -e "$PAM_LINK" ]; then
  if [ "$FORCE" -eq 1 ]; then
    remove_file "$PAM_LINK"
  else
    log "leaving non-symlink $PAM_LINK (use --force to remove)"
  fi
else
  log "PAM service already absent"
fi

if [ "$PURGE" -eq 1 ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    log "would remove ${ETC}/ursine and ${VAR}/log/ursine"
  else
    rm -rf "${ETC}/ursine" "${VAR}/log/ursine"
    log "removed ${ETC}/ursine and ${VAR}/log/ursine"
  fi
fi

if [ "$DRY_RUN" -eq 1 ]; then
  log "dry-run complete for $INIT"
else
  log "deactivated $INIT integration"
fi
