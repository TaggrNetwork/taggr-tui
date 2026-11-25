{
  lib,
  rustPlatform,
  pkg-config,
  aws-lc,
}:

rustPlatform.buildRustPackage {
  pname = "taggr-tui";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # rustls (ic-agent, reqwest) links against AWS-LC
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ aws-lc ];

  meta = {
    description = "Terminal client for Taggr, the decentralised social network";
    mainProgram = "taggr-tui";
    platforms = lib.platforms.linux;
  };
}
