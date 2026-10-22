{
  description = "Ursine — FeralOS control center (ursined daemon + GUI)";

  # Stable nixpkgs: only used for packaging (no musl toolchain build).
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = {nixpkgs, ...}: let
    system = "x86_64-linux";
    pkgs = nixpkgs.legacyPackages.${system};
    bin = import ./nix/binary.nix;

    # Package dependency chain:
    #   ursine (GUI) -> ursined (daemon) -> ursine-activate (system tools)
    # The GUI does not need the activation tools directly; installing it pulls
    # the daemon, and installing the daemon pulls the tools it uses.
    activateTool = pkgs.runCommand "ursine-activate-${bin.version}" {} ''
      mkdir -p $out/bin
      cp ${./src/tools/ursine-activate.sh} $out/bin/ursine-activate
      cp ${./src/tools/ursine-deactivate.sh} $out/bin/ursine-deactivate
      chmod 0755 $out/bin/ursine-activate $out/bin/ursine-deactivate
    '';

    # Internal unpacked release. The pinned tarball contains both binaries,
    # packaging assets, install.sh and the activation scripts.
    release = pkgs.stdenvNoCC.mkDerivation {
      pname = "ursine-release";
      inherit (bin) version;

      src = pkgs.fetchurl {
        inherit (bin) url sha256;
      };

      nativeBuildInputs = [pkgs.gnutar];
      dontUnpack = true;
      installPhase = ''
        mkdir -p $out
        tar xzf $src -C $out
      '';
    };

    daemonCore = pkgs.runCommand "ursined-core-${bin.version}" {} ''
      mkdir -p $out/bin $out/packaging
      install -Dm0755 ${release}/bin/ursined $out/bin/ursined
      cp -R ${release}/packaging/. $out/packaging/
    '';

    guiCore = pkgs.runCommand "ursine-core-${bin.version}" {} ''
      mkdir -p $out/bin
      install -Dm0755 ${release}/bin/ursine $out/bin/ursine
    '';

    # The daemon owns system activation; therefore its package carries the
    # activation/deactivation tools and the packaging assets.
    ursined = pkgs.symlinkJoin {
      pname = "ursined";
      inherit (bin) version;
      paths = [daemonCore activateTool];

      meta = {
        description = "Ursine root daemon with system activation tools (musl)";
        license = pkgs.lib.licenses.isc;
        mainProgram = "ursined";
        platforms = ["x86_64-linux"];
      };
    };

    # The desktop package depends on the daemon package, so installing the
    # GUI is sufficient for a complete Ursine installation.
    ursine = pkgs.symlinkJoin {
      pname = "ursine";
      inherit (bin) version;
      paths = [guiCore ursined];

      meta = {
        description = "Ursine control center: ursined daemon + ursine GUI (musl)";
        license = pkgs.lib.licenses.isc;
        mainProgram = "ursined";
        platforms = ["x86_64-linux"];
      };
    };
  in {
    packages.${system} = {
      default = ursine;
      inherit ursine ursined;
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
    checks.${system} = {
      package = ursine;
      daemon = ursined;
      activation = activateTool;
    };
  };
}
