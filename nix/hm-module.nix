# Home Manager module:
#   programs.taggr-tui = {
#     enable = true;
#     settings = {
#       icons = "nerd";
#       image_protocol = "kitty";
#     };
#   };
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.taggr-tui;
  toml = pkgs.formats.toml { };
in
{
  options.programs.taggr-tui = {
    enable = lib.mkEnableOption "taggr-tui, a terminal client for Taggr";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "The taggr-tui package.";
    };

    settings = lib.mkOption {
      type = toml.type;
      default = { };
      description = ''
        Contents of {file}`~/.config/taggr-tui/config.toml`; see `taggr-tui --help`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."taggr-tui/config.toml" = lib.mkIf (cfg.settings != { }) {
      source = toml.generate "taggr-tui-config.toml" cfg.settings;
    };
  };
}
