{
  description = "Ursine — FeralOS control center (ursined daemon + GUI)";

  # Stable nixpkgs: only used for packaging (no musl toolchain build).
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      bin = import ./nix/binary.nix;

      # The binaries themselves are musl (built in Alpine CI): nixpkgs does not
      # cache musl toolchains, so building from source here would take hours on
      # every run. This derivation packages the pinned release artifact — the
      # flake stays reproducible (URL + sha256) and instant to build.
      #
      # WHY dynamic musl: PAM loads pam_unix.so via dlopen at runtime; a fully
      # static musl binary cannot dlopen. FeralOS is Alpine (dynamic musl).
      ursine = pkgs.stdenvNoCC.mkDerivation {
        pname = "ursine";
        inherit (bin) version;

        src = pkgs.fetchurl {
          inherit (bin) url sha256;
        };

        nativeBuildInputs = [ pkgs.gnutar ];

        # Tarball root = bin/ (no wrapper dir): unpack straight into $out.
        dontUnpack = true;
        installPhase = ''
          mkdir -p $out
          tar xzf $src -C $out
          chmod +x $out/bin/ursined $out/bin/ursine
        '';

        meta = {
          description = "Ursine control center: ursined daemon + ursine GUI (musl)";
          license = pkgs.lib.licenses.isc;
          mainProgram = "ursined";
          platforms = [ "x86_64-linux" ];
        };
      };
    in {
      packages.${system} = {
        default = ursine;
        inherit ursine;
      };

      # `nix build --out-link artifacts/result` → artifacts/result/bin/{ursined,ursine}
      checks.${system}.package = ursine;
    };
}
