{
  description = "Fenix Rust and Native Shell development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # FounderType (方正) fonts packaged for ctex's `fontset=founder`.
    # Visual baseline of the default statement style (docs/statement-style.md §1.1).
    olymp-reference = {
      url = "github:lihaoze123/polygon-chinese-template/10f99594ff6a92e73c96f1f4ae235717049c8446";
      flake = false;
    };
    chinese-fonts-overlay = {
      url = "github:brsvh/chinese-fonts-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      fenix,
      chinese-fonts-overlay,
      olymp-reference,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      environments = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          rust = fenix.packages.${system};
          toolchain = rust.combine [
            (rust.stable.withComponents [
              "cargo"
              "rustc"
              "rustfmt"
              "clippy"
              "rust-src"
              "rust-analyzer"
            ])
            rust.targets.wasm32-unknown-unknown.stable.rust-std
            rust.targets.x86_64-pc-windows-msvc.stable.rust-std
          ];
          nsh =
            let
              inherit (pkgs)
                lib
                stdenv
                fetchurl
                autoPatchelfHook
                wrapGAppsHook4
                gtk4
                webkitgtk_6_0
                gsettings-desktop-schemas
                stdenvNoCC
                ;
              version = "0.7.1";
              releases = {
                x86_64-linux = {
                  archive = "nsh-linux-x86_64.tar.gz";
                  hash = "sha256-BEV606U8kbZu/O5TeWjFr3yQ3yXPYcGNURNezAUeIrM=";
                };
                aarch64-linux = {
                  archive = "nsh-linux-arm64.tar.gz";
                  hash = "sha256-ARCZMpNCvJYckIneLHwczdYMAXe3fYEoR1aGbCCH67s=";
                };
                aarch64-darwin = {
                  archive = "nsh-macos-arm64.tar.gz";
                  hash = "sha256-KONJtZwLRulkAQyPhYp4r1200mAS0hPAPRXOKN3/BlA=";
                };
              };
              release = releases.${stdenv.hostPlatform.system};
              releaseUrl = "https://github.com/longbridge/nsh/releases/download/v${version}";
              webRuntime = fetchurl {
                url = "${releaseUrl}/nsh-web.tar.gz";
                hash = "sha256-qC8xAid1y4gmf6V1SGZcjafbpLYvBEpJRcFzl/8LmdY=";
              };
            in
            stdenvNoCC.mkDerivation {
              pname = "nsh";
              inherit version;
              src = fetchurl {
                url = "${releaseUrl}/${release.archive}";
                inherit (release) hash;
              };

              nativeBuildInputs = lib.optionals stdenv.hostPlatform.isLinux [
                autoPatchelfHook
                wrapGAppsHook4
              ];
              buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
                gtk4
                webkitgtk_6_0
                gsettings-desktop-schemas
                stdenv.cc.cc.lib
              ];
              dontConfigure = true;
              dontBuild = true;
              # Keep the CLI beside its runtime: it resolves ../runtime from its executable.
              installPhase = ''
                runHook preInstall
                mkdir -p "$out"
                cp -r bin runtime "$out/"
                tar -xzf ${webRuntime} -C "$out" --strip-components=1
                install -Dm644 LICENSE-APACHE "$out/share/licenses/nsh/LICENSE-APACHE"
                runHook postInstall
              '';
              preFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
                # wrapGAppsHook normally wraps bin/* only; the native runners need it too.
                for runner in "$out"/runtime/linux/nsh-linux-*; do
                  wrapGApp "$runner"
                done
              '';

              meta = {
                description = "Native Shell UI framework CLI and native/web runtimes";
                homepage = "https://nsh.dev/";
                license = lib.licenses.asl20;
                platforms = builtins.attrNames releases;
                mainProgram = "nsh";
                sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
              };
            };
          # Statement typesetting fonts, pinned so that the Typst engine and the
          # olymp.sty reference PDFs (built with tectonic) read the very same files.
          statementFonts = [
            pkgs.lmodern
            pkgs.jetbrains-mono
            chinese-fonts-overlay.packages.${system}.foundertype-ctex-fonts
          ];
          # Only the pinned fonts: no /etc/fonts, no user or profile directories.
          statementFontsConf = pkgs.writeText "xcpc-statement-fonts.conf" ''
            <?xml version="1.0"?>
            <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
            <fontconfig>
              ${pkgs.lib.concatMapStrings (f: "<dir>${f}/share/fonts</dir>\n") statementFonts}
              <cachedir prefix="xdg">xcpc-statement-fontconfig</cachedir>
            </fontconfig>
          '';
          linuxLibraries = with pkgs; [
            gtk4
            webkitgtk_6_0
          ];
        in
        {
          inherit nsh;
          shell = pkgs.mkShell {
            nativeBuildInputs = [ pkgs.rustPlatform.bindgenHook ];
            packages = [
              toolchain
              nsh
              pkgs.pkg-config
              pkgs.clang
              pkgs.cmake
              pkgs.cargo-xwin
              pkgs.lld
              pkgs.llvm
              pkgs.upx
              pkgs.tectonic
              pkgs.pandoc
              pkgs.poppler-utils
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.gobject-introspection ];
            buildInputs =
              pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux linuxLibraries
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];

            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            # Font directories for the statement engine (colon-separated).
            XCPC_FONT_DIRS = pkgs.lib.makeSearchPath "share/fonts" statementFonts;
            # Fontconfig restricted to the statement fonts: set it as FONTCONFIG_FILE
            # only when building reference PDFs, never for the whole shell (the GUI needs system fonts).
            XCPC_FONTCONFIG_FILE = "${statementFontsConf}";
            # Pinned polygon-chinese-template checkout used to build reference PDFs.
            XCPC_OLYMP_REFERENCE = "${olymp-reference}";

            shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              export XDG_DATA_DIRS="${pkgs.gtk4}/share:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
              # Use lld for much faster Linux incremental linking while keeping Nix's
              # wrapped `cc` so all Nix library rpaths (GTK4, OpenSSL, etc.) are preserved.
              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld"
            '';
          };
        }
      );
    in
    {
      devShells = forAllSystems (system: {
        default = environments.${system}.shell;
      });
      packages = forAllSystems (system: {
        inherit (environments.${system}) nsh;
        default = environments.${system}.nsh;
      });
      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
