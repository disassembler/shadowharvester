{
  flake = { config, ... }: {
    nixosModules = let
      name = "shadow-harvester";
    in {
      default = {
        imports = [
          config.nixosModules.${name}
        ];

        nixpkgs.overlays = [
          config.overlays.default
        ];
      };

      ${name} = { config, lib, pkgs, ... }: let
        cfg = config.services.${name};
      in {
        options.services.${name} = {
          enable = lib.mkEnableOption name;

          package = lib.mkPackageOption pkgs name {};

          settings = lib.mkOption {
            type = lib.types.submodule {
              freeformType = with lib.types; attrsOf (oneOf [
                str bool ints.u32
              ]);

              options.api-url = lib.mkOption {
                type = lib.types.str;
                default = "https://sm.midnight.gd/api";
              };
            };
            default = {};
          };
        };

        config = lib.mkIf cfg.enable {
          systemd.services.${name} = {
            after = ["network.target"];
            wantedBy = ["multi-user.target"];

            path = [cfg.package];

            script = toString [
              "exec"
              (with lib; pipe cfg.package [getExe builtins.baseNameOf escapeShellArg])
              (lib.cli.toGNUCommandLineShell {} cfg.settings)
            ];

            enableStrictShellChecks = true;

            serviceConfig = {
              Type = "exec";
              DynamicUser = true;
            };
          };
        };
      };
    };
  };
}
