{
  description = "simkl-mcp development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));
    in {
      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            bun
            # wrangler runs on node
            nodejs_22
            # the Rust rewrite in crates/
            cargo
            clippy
            rustc
            rustfmt
            rust-analyzer
            gh
          ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];

          WRANGLER_SEND_TELEMETRY = "false";
          RUST_LOG = "simkl_mcp=debug,simkl_api=debug,tower_http=info";
          shellHook = ''
            echo "simkl-mcp dev shell: bun $(bun --version), node $(node --version), rust $(rustc --version | cut -d' ' -f2)"
          '';
        };
      });
    };
}
