{
    description = "Audio-reactive terminal visualiser";

    inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    outputs = { self, nixpkgs }:
        let
            forSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
        in {
            packages = forSystems (system:
                let
                    pkgs = nixpkgs.legacyPackages.${system};
                in rec {
                    vibe = pkgs.rustPlatform.buildRustPackage {
                        pname = "vibe";
                        version = "0.1.0";
                        src = nixpkgs.lib.cleanSource self;
                        cargoLock.lockFile = ./Cargo.lock;
                        nativeBuildInputs = [ pkgs.makeWrapper ];
                        postInstall = ''
                            wrapProgram $out/bin/vibe \
                                --prefix PATH : ${nixpkgs.lib.makeBinPath [ pkgs.pipewire ]}
                        '';
                        meta = {
                            description = "Audio-reactive terminal visualiser";
                            mainProgram = "vibe";
                            platforms = nixpkgs.lib.platforms.linux;
                        };
                    };
                    default = vibe;
                });
        };
}
