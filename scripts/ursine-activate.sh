#!/bin/sh
# Ursine system activation for Nix profile installs.
#
# Run as root after installing or upgrading the Nix profile:
#   nix profile add --profile /nix/var/nix/profiles/ursine \
#     github:skea999/ursine#ursine
#   sudo /nix/var/nix/profiles/ursine/bin/ursine-activate
#
# The script is idempotent. Re-run it after every upgrade so changed PAM or
# init-unit assets are applied; the service always points at the stable profile
# path, so the upgraded binary is used after the restart.
set -eu

PROFILE=${URSINE_PROFILE:-}
if [ -z "$PROFILE" ]; then
    if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
        user_home=$(getent passwd "$SUDO_USER" 2>/dev/null | cut -d: -f6) || user_home=
        [ -n "$user_home" ] || user_home="/home/$SUDO_USER"
        if [ -x "$user_home/.nix-profile/bin/ursined" ]; then
            PROFILE="$user_home/.nix-profile"
        fi
    fi
    if [ -z "$PROFILE" ] && [ -x "${HOME:-}/.nix-profile/bin/ursined" ]; then
        PROFILE="${HOME}/.nix-profile"
    fi
    [ -n "$PROFILE" ] || PROFILE=/nix/var/nix/profiles/ursine
fi
ROOT=
INIT=
START=1
FORCE=0
DRY_RUN=0
CHANGED=0

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
  --force          replace an existing non-symlink /etc/pam.d/ursine
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
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
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
[ -x "$PROFILE/bin/ursined" ] || die "missing executable: $PROFILE/bin/ursined"
[ -f "$PROFILE/packaging/pam.d/ursine" ] || die "missing PAM asset: $PROFILE/packaging/pam.d/ursine"

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
    START=0
elif [ "$(id -u)" -ne 0 ]; then
    die "must run as root (use --root for staged tests)"
fi

[ "$DRY_RUN" -eq 1 ] && START=0

if [ -z "$INIT" ]; then
    if [ -d /run/systemd/system ] || command -v systemctl >/dev/null 2>&1; then
        INIT=systemd
    elif command -v rc-service >/dev/null 2>&1; then
        INIT=openrc
    elif command -v dinitctl >/dev/null 2>&1 || [ -d /etc/dinit.d ]; then
        INIT=dinit
    else
        die "cannot detect init; use --init systemd|openrc|dinit"
    fi
fi

case "$INIT" in
    systemd|openrc|dinit) ;;
    *) die "unsupported init: $INIT" ;;
esac

ETC="${ROOT}/etc"
VAR="${ROOT}/var"
LIBEXEC="${ROOT}/usr/lib"
PAM_LINK="$ETC/pam.d/ursine"
PAM_TARGET="$PROFILE/packaging/pam.d/ursine"
SYSTEMD_UNIT="$ETC/systemd/system/ursined.service"
OPENRC_INIT="$ETC/init.d/ursined"
DINIT_UNIT="${ROOT}/lib/dinit.d/ursined"
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
    tmp="${dst}.tmp.$$"
    install -m "$mode" "$src" "$tmp"
    if [ -f "$dst" ] && cmp -s "$tmp" "$dst"; then
        rm -f "$tmp"
    else
        mv "$tmp" "$dst"
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
    tmp="${dst}.tmp.$$"
    sed \
        -e "s|@SBINDIR@|$PROFILE/bin|g" \
        -e "s|@SYSCONFDIR@|$ETC|g" \
        -e "s|@LOCALSTATEDIR@|$VAR|g" \
        -e "s|@LIBEXECDIR@|$LIBEXEC|g" \
        -e "s|@DINIT_CONFIG_DIR@|$ETC/dinit.d/config|g" \
        "$src" > "$tmp"
    chmod "$mode" "$tmp"
    if [ -f "$dst" ] && cmp -s "$tmp" "$dst"; then
        rm -f "$tmp"
    else
        mv "$tmp" "$dst"
        CHANGED=1
    fi
}

install_pam_symlink() {
    ensure_dir "$(dirname "$PAM_LINK")"
    if [ "$DRY_RUN" -eq 1 ]; then
        log "would link $PAM_LINK -> $PAM_TARGET"
        return
    fi
    if [ -L "$PAM_LINK" ] && [ "$(readlink "$PAM_LINK")" = "$PAM_TARGET" ]; then
        return
    fi
    if [ -e "$PAM_LINK" ] && [ ! -L "$PAM_LINK" ] && [ "$FORCE" -ne 1 ]; then
        die "$PAM_LINK exists and is not a symlink; use --force to replace it"
    fi
    ln -sfn "$PAM_TARGET" "$PAM_LINK"
    CHANGED=1
}

ensure_dir "$ETC"
ensure_dir "$ETC/ursine"
ensure_dir "$VAR/log/ursine"
if [ "$DRY_RUN" -eq 0 ]; then
    chmod 0750 "$ETC/ursine" "$VAR/log/ursine"
fi

install_pam_symlink

case "$INIT" in
    systemd)
        render_file "$PROFILE/packaging/systemd/ursined.service" "$SYSTEMD_UNIT" 0644
        ;;
    openrc)
        render_file "$PROFILE/packaging/openrc/ursined" "$OPENRC_INIT" 0755
        ;;
    dinit)
        render_file "$PROFILE/packaging/dinit/ursined" "$DINIT_UNIT" 0644
        render_file "$PROFILE/packaging/dinit/ursined.wrapper" "$DINIT_WRAPPER" 0755
        install_file "$PROFILE/packaging/dinit/ursined.conf" "$DINIT_CONFIG" 0644
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
            dinitctl enable ursined 2>/dev/null || true
            dinitctl restart ursined 2>/dev/null || dinitctl start ursined
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
