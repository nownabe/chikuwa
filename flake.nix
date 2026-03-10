{
  description = "tmux AI Agent monitor TUI";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "chikuwa";
        version = "0.1.7";

        src = ./.;

        cargoLock.lockFile = ./Cargo.lock;

        meta = with pkgs.lib; {
          description = "tmux AI Agent monitor TUI";
          homepage = "https://github.com/nownabe/chikuwa";
          license = licenses.asl20;
          mainProgram = "chikuwa";
          platforms = [ "x86_64-linux" ];
        };
      };
    };
}
