# Local development flake. Copied to artifacts/ursine-dev/flake.nix by
# scripts/build-gui-dev.sh, next to the freshly built bin/ directory.
#
# This packages the current working-tree binaries for local testing only. The
# release flake (root flake.nix) still packages the pinned, checksummed tarball.
{
  description = "Ursine local development build (musl binaries from the dev container)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      ursine = pkgs.stdenvNoCC.mkDerivation {
        pname = "ursine-dev";
        version = "0.1.0-dev";

        # bin/, packaging/ and install.sh sit next to this file (staged by
        # scripts/build-gui-dev.sh).
        src = ./.;

        dontUnpack = true;
        installPhase = ''
          mkdir -p $out
          cp -r $src/bin $out/bin
          cp -r $src/packaging $out/packaging
          cp $src/install.sh $out/install.sh
          chmod +x $out/bin/ursined $out/bin/ursine $out/install.sh
        '';

        meta = {
          description = "Ursine development build (dynamic musl; run on Alpine/FeralOS)";
          mainProgram = "ursine";
          platforms = [ "x86_64-linux" ];
        };
      };
    in {
      packages.${system} = {
        default = ursine;
        inherit ursine;
      };

      apps.${system} = {
        default = {
          type = "app";
          program = "${ursine}/bin/ursine";
        };
        ursine = {
          type = "app";
          program = "${ursine}/bin/ursine";
        };
        ursined = {
          type = "app";
          program = "${ursine}/bin/ursined";
        };
        activate = {
          type = "app";
          program = "${ursine}/bin/ursine-activate";
        };
        deactivate = {
          type = "app";
          program = "${ursine}/bin/ursine-deactivate";
        };
      };
    };
}
