{
  description = "Plow SMI — GPU & system monitoring suite";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      # Every binary the workspace produces, keyed by its actual bin name,
      # each pointing back at the cargo package (crate) that builds it.
      binaries = {
        "plow-smi" = {
          cargoPackage = "plows-cli";
          description = "Unified Plow SMI CLI — export, monitor, and control GPUs from one binary";
        };
        "plows-exporter" = {
          cargoPackage = "plows-exporter";
          description = "Professional GPU metrics exporter for Prometheus (AMD, NVIDIA, Intel, TPU, System)";
        };
        "plows-top" = {
          cargoPackage = "plows-top";
          description = "Professional terminal GPU & system monitor — like htop for GPUs";
        };
        "plows-ctl" = {
          cargoPackage = "plows-ctl";
          description = "GPU power & clock control library & CLI — backed by plows-gpu (NVML / AMD SMI)";
        };
      };
      version = "0.1.0";
      homepage = "https://github.com/infervisor/plow-smi";
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        commonMeta = with pkgs.lib; {
          inherit homepage;
          license = licenses.asl20;
          platforms = platforms.linux;
        };

        # One build compiles the whole workspace; every binary lands in
        # $out/bin. Individual per-binary packages below slice this apart so
        # `nix build .#<name>` never has to recompile the tree from scratch.
        workspace = pkgs.rustPlatform.buildRustPackage {
          pname = "plow-smi-workspace";
          inherit version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];

          cargoBuildFlags = [ "--workspace" "--bins" ];
          cargoTestFlags = [ "--workspace" ];

          meta = commonMeta // {
            description = "Plow SMI — GPU & system monitoring suite (all binaries)";
          };
        };

        mkBinPackage = name: { cargoPackage, description }:
          pkgs.runCommand name
            {
              meta = commonMeta // {
                inherit description;
                mainProgram = name;
              };
              passthru = { inherit cargoPackage; };
            } ''
            mkdir -p $out/bin
            install -m755 ${workspace}/bin/${name} $out/bin/${name}
          '';

        binPackages = pkgs.lib.mapAttrs mkBinPackage binaries;

        mkApp = name: pkg: {
          type = "app";
          program = "${pkg}/bin/${name}";
          meta = { description = pkg.meta.description; };
        };
      in
      {
        packages = binPackages // {
          default = binPackages."plow-smi";
          # Full workspace build: every binary in one derivation
          # (`nix build .#all`), useful for images/CI artifact bundling.
          all = workspace;
        };

        apps = (pkgs.lib.mapAttrs mkApp binPackages) // {
          default = mkApp "plow-smi" binPackages."plow-smi";
        };

        checks = binPackages // {
          workspace-build-and-test = workspace;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.rust-analyzer
            pkgs.pkg-config
          ];
        };
      }
    ) // {
      nixosModules.default = import ./nix/module.nix self;
      nixosModules.plow-smi-exporter = self.nixosModules.default;

      overlays.default = final: prev: {
        inherit (self.packages.${final.system})
          plow-smi
          plows-exporter
          plows-top
          plows-ctl;
      };
    };
}
