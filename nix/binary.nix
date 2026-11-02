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
  version = "0.25.1";
  url = "https://github.com/skea999/ursine/releases/download/v0.25.1/ursine-v0.25.1-x86_64-musl.tar.gz";
  # sha256 of the tarball (hex, as printed by sha256sum)
  sha256 = "1dc0c8c89929a14f433030a03e68e2a8f90e0fe7b15d61a45fd7c6ea8ee6f6c2";
}
