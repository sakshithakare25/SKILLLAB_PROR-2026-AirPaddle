{
  pkgs ? import <nixpkgs> { },
}:

let
  isCross = pkgs.stdenv.buildPlatform != pkgs.stdenv.hostPlatform;
  rustTarget = pkgs.stdenv.hostPlatform.rust.rustcTarget;
  targetEnvVar = pkgs.lib.replaceStrings [ "-" ] [ "_" ] (pkgs.lib.toUpper rustTarget);
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs.buildPackages; [
    cargo
    rustc
    pkg-config
    cmake
  ];

  buildInputs = with pkgs; [
    raylib
    alsa-lib
    libx11
    libxrandr
    libxinerama
    libxcursor
    libxi
    libglvnd
    mesa
    llvmPackages.libclang
  ];

  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
  LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
    with pkgs;
    [
      libglvnd
      libx11
      libxrandr
      libxinerama
      libxcursor
      libxi
    ]
  );

  shellHook = pkgs.lib.optionalString isCross ''
    export CARGO_TARGET_${targetEnvVar}_LINKER="${pkgs.stdenv.cc.targetPrefix}cc"
    export PKG_CONFIG_ALLOW_CROSS="1"
  '';
}
