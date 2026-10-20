{
  description = "Ursine — FeralOS control center (ursined daemon + GUI)";

  # Stable nixpkgs: only used for packaging (no musl toolchain build).
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      bin = import ./nix/binary.nix;

      # Versioned activation/deactivation tools. They are a dependency of the
      # ursine package, so `nix profile upgrade ursine` updates them together
      # with the binaries.
      activateTool = pkgs.runCommand "ursine-activate-${bin.version}" { } ''
        mkdir -p $out/bin
        cp ${./scripts/ursine-activate.sh} $out/bin/ursine-activate
        cp ${./scripts/ursine-deactivate.sh} $out/bin/ursine-deactivate
        chmod 0755 $out/bin/ursine-activate $out/bin/ursine-deactivate
      '';

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
          if [ -e $out/bin/ursine-activate ]; then
            chmod +x $out/bin/ursine-activate
          fi
          ln -sfn ${activateTool}/bin/ursine-activate $out/bin/ursine-activate
          ln -sfn ${activateTool}/bin/ursine-deactivate $out/bin/ursine-deactivate
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
        "ursine-activate" = activateTool;
      };

      apps.${system} = {
        activate = {
          type = "app";
          program = "${activateTool}/bin/ursine-activate";
        };
        deactivate = {
          type = "app";
          program = "${activateTool}/bin/ursine-deactivate";
        };
      };

      # `nix build --out-link artifacts/result` → artifacts/result/bin/{ursined,ursine}
      checks.${system}.package = ursine;
    };
}
