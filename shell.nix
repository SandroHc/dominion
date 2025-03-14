# Shell for Xilem development
# Based on:
#  - https://github.com/emilk/egui/discussions/1587#discussioncomment-2698797
#  - https://github.com/linebender/xilem/issues/66
#
# Patching generated binary to run in Nix:
#   cargo build && patchelf --set-rpath "$(patchelf --print-rpath ./target/debug/michi):${LD_LIBRARY_PATH}" ./target/debug/michi
let
  rustOverlay = import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz");
  pkgs = import <nixpkgs> {overlays = [rustOverlay];};
  rustToolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [
      "rust-src"
    ];
  };
in
  pkgs.mkShell rec {
    nativeBuildInputs = with pkgs; [
      rustToolchain
      pkg-config
      openssl
    ];

    buildInputs = with pkgs; [
    ];

    LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
  }
