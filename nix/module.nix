{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    mkPackageOption
    types
    ;

  cfg = config.services.barp;

  displayOptionsType = types.submodule {
    options = {
      shader = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "EmulatorJS shader setting, such as `disabled` or `crt-mattias.glslp`.";
      };
      smooth = mkOption {
        type = types.nullOr types.bool;
        default = null;
        description = "Whether to use smooth browser upscaling for the game canvas.";
      };
      integerScale = mkOption {
        type = types.nullOr types.bool;
        default = null;
        description = "Whether to size the canvas to an integer multiple of native resolution.";
      };
    };
  };

  toJsonOptions = opts: lib.filterAttrs (_: value: value != null) {
    inherit (opts) shader smooth;
    integer_scale = opts.integerScale;
  };

  settings = {
    roms_path = toString cfg.romsPath;
    saves_path = toString cfg.savesPath;
    emulatorjs_path = toString cfg.emulatorjsPackage;
    port = cfg.port;
    default_options = toJsonOptions cfg.defaultOptions;
    system_mappings = cfg.systemMappings;
    users = lib.mapAttrs (
      _username: user: {
        password_hash_file = toString user.passwordHashFile;
        option_overrides = toJsonOptions user.optionOverrides;
      }
    ) cfg.users;
  };

  configFile = pkgs.writeText "barp.json" (builtins.toJSON settings);

  passwordHashFiles = lib.mapAttrsToList (_: user: toString user.passwordHashFile) cfg.users;
in
{
  options.services.barp = {
    enable = mkEnableOption "BARP (Boring Ahh ROM Player)";

    package = mkPackageOption pkgs "barp" { };

    emulatorjsPackage = mkPackageOption pkgs "barp-emulatorjs" { };

    romsPath = mkOption {
      type = types.path;
      example = "/var/lib/roms";
      description = "Directory containing ROM folders. Mounted read-only into the service.";
    };

    savesPath = mkOption {
      type = types.path;
      default = "/var/lib/barp/saves";
      description = "Directory for per-user save data.";
    };

    port = mkOption {
      type = types.port;
      default = 3000;
      description = "TCP port for the BARP HTTP server.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to open `services.barp.port` in the firewall.";
    };

    defaultOptions = mkOption {
      type = displayOptionsType;
      default = {
        shader = "disabled";
        smooth = false;
        integerScale = false;
      };
      description = "Default display options applied before per-user overrides.";
    };

    systemMappings = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = {
        my-ps1-games = "psx";
        accurate-ps1 = "mednafen_psx_hw";
      };
      description = ''
        Map top-level ROM folder names to EmulatorJS system names or concrete
        core names. These override built-in folder aliases.
      '';
    };

    users = mkOption {
      type = types.attrsOf (
        types.submodule (
          { ... }:
          {
            options = {
              passwordHashFile = mkOption {
                type = types.path;
                description = ''
                  Path to a file containing an Argon2 PHC password hash
                  (Argon2id recommended). See the README for argon2 CLI /
                  argon2.online generator settings.
                '';
              };
              optionOverrides = mkOption {
                type = displayOptionsType;
                default = { };
                description = "Per-user display option overrides.";
              };
            };
          }
        )
      );
      default = { };
      description = "BARP users keyed by username.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.users != { };
        message = "services.barp.users must declare at least one user.";
      }
    ];

    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall [ cfg.port ];

    systemd.services.barp = {
      description = "BARP (Boring Ahh ROM Player)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} --config ${configFile}";
        Restart = "on-failure";

        DynamicUser = true;
        StateDirectory = "barp";
        UMask = "0077";

        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        LockPersonality = true;
        MemoryDenyWriteExecute = true;

        ReadOnlyPaths = [ cfg.romsPath cfg.emulatorjsPackage ] ++ passwordHashFiles;
        ReadWritePaths = [ cfg.savesPath ];
      };
    };
  };
}
