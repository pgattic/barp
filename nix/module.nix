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

  playerOptionsType = types.submodule {
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
      fourScore = mkOption {
        type = types.nullOr types.bool;
        default = null;
        description = ''
          Whether to force the NES Four Score on, connecting players 3 and 4 in
          every NES game. Without it the core only enables the adapter for games
          in its CRC database, so homebrew such as Micro Mages stays 2-player.
          Ignored by every other system.
        '';
      };
    };
  };

  toJsonOptions = opts: lib.filterAttrs (_: value: value != null) {
    inherit (opts) shader smooth;
    integer_scale = opts.integerScale;
    four_score = opts.fourScore;
  };

  settings = {
    roms_path = toString cfg.romsPath;
    saves_path = toString cfg.savesPath;
    emulatorjs_path = toString cfg.emulatorjsPackage;
    port = cfg.port;
    default_options = toJsonOptions cfg.defaultOptions;
    system_mappings = cfg.systemMappings;
    users = lib.mapAttrs (
      _username: user:
      lib.filterAttrs (_: value: value != null) {
        password_hash = user.passwordHash;
        password_hash_file =
          if user.passwordHashFile == null then null else toString user.passwordHashFile;
        option_overrides = toJsonOptions user.optionOverrides;
      }
    ) cfg.users;
  };

  configFile = pkgs.writeText "barp.json" (builtins.toJSON settings);

  passwordHashFiles = lib.mapAttrsToList (_: user: toString user.passwordHashFile) (
    lib.filterAttrs (_: user: user.passwordHashFile != null) cfg.users
  );

  userPasswordAssertions = lib.mapAttrsToList (username: user: {
    assertion = (user.passwordHash != null) != (user.passwordHashFile != null);
    message = ''
      services.barp.users.${username} must set exactly one of passwordHash or
      passwordHashFile.
    '';
  }) cfg.users;
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
      type = playerOptionsType;
      default = {
        shader = "disabled";
        smooth = false;
        integerScale = false;
        fourScore = false;
      };
      description = "Default player options applied before per-user overrides.";
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
              passwordHash = mkOption {
                type = types.nullOr types.str;
                default = null;
                description = ''
                  Inline Argon2 PHC password hash (Argon2id recommended).
                  Mutually exclusive with `passwordHashFile`.

                  Discouraged for production: the hash is written into the
                  world-readable Nix store. Prefer `passwordHashFile` with
                  agenix/sops for real deployments.
                '';
              };
              passwordHashFile = mkOption {
                type = types.nullOr types.path;
                default = null;
                description = ''
                  Path to a file containing an Argon2 PHC password hash
                  (Argon2id recommended). Mutually exclusive with
                  `passwordHash`. Prefer this for production so hashes stay
                  out of the Nix store. See the README for argon2 CLI /
                  argon2.online generator settings.
                '';
              };
              optionOverrides = mkOption {
                type = playerOptionsType;
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
    ]
    ++ userPasswordAssertions;

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
