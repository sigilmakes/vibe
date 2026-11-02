{
  description = "Audio-reactive terminal visualiser";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      forSystems = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    {
      packages = forSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        rec {
          vibe = pkgs.rustPlatform.buildRustPackage {
            pname = "vibe";
            version = "0.1.0";
            src = nixpkgs.lib.cleanSourceWith {
              src = ./.;
              filter =
                path: _:
                !(builtins.elem (baseNameOf path) [
                  ".git"
                  "result"
                  "screenshots"
                  "target"
                ]);
            };
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.makeWrapper ];
            postInstall = ''
              wrapProgram $out/bin/vibe \
                  --prefix PATH : ${nixpkgs.lib.makeBinPath [ pkgs.pipewire pkgs.pulseaudio ]}
            '';
            meta = {
              description = "Audio-reactive terminal visualiser";
              mainProgram = "vibe";
              platforms = nixpkgs.lib.platforms.linux;
            };
          };
          default = vibe;
        }
      );

      apps = forSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/vibe";
        };
      });

      devShells = forSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              pkg-config
              pipewire
              pulseaudio
              rustc
              rustfmt
            ];
          };
        }
      );

      checks = forSystems (system: {
        default = self.packages.${system}.default;
      });

      formatter = forSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-rfc-style);
    };
}
