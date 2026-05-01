{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          crossSystem = if system == "x86_64-linux" then "aarch64-linux" else null;

          pkgsCross =
            if crossSystem != null then
              import nixpkgs {
                inherit system;
                crossSystem = {
                  config = "aarch64-unknown-linux-gnu";
                };
              }
            else
              null;
        in
        {
          default = import ./shell.nix { inherit pkgs; };
        }
        // pkgs.lib.optionalAttrs (pkgsCross != null) {
          cross = import ./shell.nix { pkgs = pkgsCross; };
        }
      );
    };
}
