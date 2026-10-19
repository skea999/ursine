{
  description = "Ursine — FeralOS control center (ursined daemon + GUI)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      # musl everywhere (binding rule): pkgsMusl, dynamic musl binaries
      # (Alpine-style; static musl cannot link libpam). Never glibc.
      pkgs = nixpkgs.legacyPackages.${system};
      musl = pkgs.pkgsMusl;
      version = "0.1.0";
    in {
      packages.${system} = {
        default = self.packages.${system}.ursine;

        ursine = musl.rustPlatform.buildRustPackage {
          pname = "ursine";
          inherit version;
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          # Real PAM on release builds (linux-pam is on every FeralOS install).
          buildFeatures = [ "pam" ];
          buildInputs = [ musl.linux-pam ];

          # GUI crate is a placeholder until the store milestone; no GTK yet.
          doCheck = true;

          meta = {
            description = "Ursine control center: ursined daemon + ursine GUI (musl)";
            license = pkgs.lib.licenses.isc;
            mainProgram = "ursined";
            platforms = [ "x86_64-linux" ];
          };
        };
      };

      # `nix build --out-link artifacts/result` builds both binaries at:
      #   artifacts/result/bin/ursined, artifacts/result/bin/ursine
      checks.${system}.build = self.packages.${system}.ursine;
    };
}
