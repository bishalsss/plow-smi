{ pkgs, workspace, version, system, src }:

let
  # Corresponding runtime sources, patches and build recipes for redistribution.
  runtimeSources = pkgs.runCommand "plow-smi-runtime-sources-${version}-${system}"
    { nativeBuildInputs = [ pkgs.gnutar pkgs.gzip pkgs.xz ]; }
    ''
      mkdir -p sources/upstream sources/patches/glibc sources/patches/gcc "$out"
      cp ${pkgs.glibc.src} sources/upstream/glibc.tar.xz
      cp ${pkgs.stdenv.cc.cc.src} sources/upstream/gcc.tar.xz
      ${pkgs.lib.concatMapStringsSep "\n" (p: "cp ${p} sources/patches/glibc/") (pkgs.glibc.patches or [])}
      ${pkgs.lib.concatMapStringsSep "\n" (p: "cp ${p} sources/patches/gcc/") (pkgs.stdenv.cc.cc.patches or [])}
      cp -R ${pkgs.path} sources/nixpkgs
      # Include the application's build recipes too: flake.nix imports files
      # under nix/ and scripts/, so copying just the flake is insufficient.
      cp -R ${src} sources/plow-smi
      mkdir -p "$out/licenses"
      tar -xOf ${pkgs.glibc.src} --wildcards --no-wildcards-match-slash '*/COPYING' > "$out/licenses/glibc-COPYING"
      tar -xOf ${pkgs.glibc.src} --wildcards --no-wildcards-match-slash '*/COPYING.LIB' > "$out/licenses/glibc-COPYING.LIB"
      tar -xOf ${pkgs.stdenv.cc.cc.src} --wildcards --no-wildcards-match-slash '*/COPYING3' > "$out/licenses/gcc-COPYING3"
      tar -xOf ${pkgs.stdenv.cc.cc.src} --wildcards --no-wildcards-match-slash '*/COPYING.RUNTIME' > "$out/licenses/gcc-COPYING.RUNTIME"
      cp -R "$out/licenses" sources/licenses
      tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
        -cf - sources | gzip -n > "$out/plow-smi-${version}-${system}-runtime-sources.tar.gz"
    '';
in

pkgs.runCommand "plow-smi-${version}-${system}-release"
  {
    nativeBuildInputs = [ pkgs.python3 ]
      ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.binutils pkgs.patchelf ]
      ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.darwin.cctools pkgs.darwin.sigtool ];
    meta = workspace.meta // {
      description = "Portable Plow SMI release archive (all binaries)";
    };
  }
  ''
    python3 ${src}/scripts/package-release.py \
      --workspace ${workspace} --source ${src} \
      --version ${version} --system ${system} --output "$out" \
      ${pkgs.lib.optionalString pkgs.stdenv.isLinux "--glibc ${pkgs.glibc} --gcc-lib ${pkgs.stdenv.cc.cc.lib} --runtime-sources ${runtimeSources}"}
  ''
