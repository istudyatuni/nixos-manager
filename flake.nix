{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

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
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = libraries;

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries;
      };
    };
}
