{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      cargoPackage = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;

      libraries = with pkgs; [
        # Wayland / X11
        wayland
        libxkbcommon
        libx11
        libxcursor
        libxi
        libxrandr
        libxcb

        # Graphics
        libGL
        vulkan-loader

        # Fonts
        fontconfig
        freetype
      ];
    in
    {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = cargoPackage.name;
        inherit (cargoPackage) version;
        src = pkgs.lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "--workspace" ];
        cargoTestFlags = [ "--workspace" ];
        env.NIXOS_MANAGER_PACKAGED = "1";

        nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper ];
        buildInputs = libraries;

        postInstall = ''
          install -Dm755 "$out/bin/nixos-manager-apply" "$out/libexec/nixos-manager-apply"
          rm "$out/bin/nixos-manager-apply"
          install -Dm644 static/packaging/nixos-manager.desktop \
            "$out/share/applications/nixos-manager.desktop"
          wrapProgram "$out/bin/nixos-manager" \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath libraries}"
        '';
      };

      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = libraries;

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries;
      };
    };
}
