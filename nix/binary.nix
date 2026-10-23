# Pinned musl release artifact consumed by flake.nix.
#
# WHY a prebuilt tarball: nixpkgs does not cache pkgsMusl/pkgsStatic tools
# (stable included) — a source build would compile the musl toolchain for
# hours on every CI run. musl dynamic binaries must link libpam at runtime
# (PAM modules are dlopen'ed; static musl has no dlopen), so the artifact is
# built in the Alpine CI container (same libc, fast) and pinned here.
#
# RELEASE CHECKLIST: CI updates version/url/sha256 on every `v*` tag; keep this
# as the manual fallback.
#   sha256sum ursine-<ver>-x86_64-musl.tar.gz
{
  version = "0.6.0";
  url = "https://github.com/skea999/ursine/releases/download/v0.6.0/ursine-v0.6.0-x86_64-musl.tar.gz";
  # sha256 of the tarball (hex, as printed by sha256sum)
  sha256 = "90bc57108b28f2e3f967a1da7dfd650e17444eaae49a5241a44ddb6b9032c70b";
}
