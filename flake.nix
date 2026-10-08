{
  description = "Alethiometer devshell";

  inputs.devshell.url = "github:numtide/devshell";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";

  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };

  outputs =
    {
      self,
      flake-utils,
      devshell,
      nixpkgs,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (system: {
      devShells.default =
        let
          pkgs = import nixpkgs {
            inherit system;

            overlays = [ devshell.overlays.default ];
          };
        in
        pkgs.devshell.mkShell {
          imports = [
            (pkgs.devshell.extraModulesPath + "/language/rust.nix")
            (pkgs.devshell.importTOML ./devshell.toml)
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            (pkgs.devshell.extraModulesPath + "/language/c.nix")
            (
              let
                libs = with pkgs; [
                  openssl
                  dbus
                  glib
                  gtk3
                  webkitgtk_4_1
                  libsoup_3
                  cairo
                  pango
                  gdk-pixbuf
                  atk
                  harfbuzz
                  zlib
                ];
              in
              {
                language.c = {
                  compiler = pkgs.gcc;
                  includes = libs;
                  libraries = libs;
                };
                devshell.packages = [ pkgs.mold ];
                env = [
                  {
                    name = "CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_RUSTFLAGS";
                    value = "-C link-arg=-fuse-ld=mold";
                  }
                  {
                    name = "PKG_CONFIG_PATH";
                    prefix = "$DEVSHELL_DIR/share/pkgconfig";
                  }
                  {
                    name = "__EGL_VENDOR_LIBRARY_DIRS";
                    value = "${pkgs.mesa}/share/glvnd/egl_vendor.d";
                  }
                  {
                    name = "GBM_BACKENDS_PATH";
                    value = "${pkgs.mesa}/lib/gbm";
                  }
                  {
                    name = "LIBGL_DRIVERS_PATH";
                    value = "${pkgs.mesa}/lib/dri";
                  }
                ];
              }
            )
          ];
        };
    });
}
