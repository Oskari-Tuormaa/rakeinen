{
  description = "Rakeinen - a live image dithering GUI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      runtimeLibs =
        pkgs: with pkgs; [
          wayland
          libxkbcommon
          libGL
          libx11
          libxcursor
          libxi
          libxrandr
        ];
    in
    {
      packages = forAllSystems (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "rakeinen";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = runtimeLibs pkgs;

          postInstall = ''
            install -Dm644 packaging/rakeinen.desktop $out/share/applications/rakeinen.desktop
            install -Dm644 packaging/rakeinen.svg $out/share/icons/hicolor/scalable/apps/rakeinen.svg
            substituteInPlace $out/share/applications/rakeinen.desktop \
              --replace-fail "Exec=rakeinen" "Exec=$out/bin/rakeinen"
          '';

          postFixup = ''
            patchelf --add-rpath ${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)} $out/bin/rakeinen
          '';

          meta = {
            description = "Live image dithering GUI";
            mainProgram = "rakeinen";
          };
        };
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/rakeinen";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
            pkg-config
          ];

          buildInputs = runtimeLibs pkgs;

          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs pkgs);
        };
      });

      checks = forAllSystems (pkgs: {
        desktop-entry =
          pkgs.runCommand "validate-desktop-entry" { nativeBuildInputs = [ pkgs.desktop-file-utils ]; }
            ''
              desktop-file-validate ${./packaging/rakeinen.desktop}
              touch $out
            '';
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-rfc-style);
    };
}
