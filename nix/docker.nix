{
  dockerTools,
  barp,
  emulatorjs,
}:

dockerTools.buildLayeredImage {
  name = "ghcr.io/pgattic/barp";
  tag = barp.version or "0.1.0";

  contents = [
    barp
    dockerTools.usrBinEnv
    dockerTools.binSh
    dockerTools.fakeNss
  ];

  # EmulatorJS at a stable /emulatorjs path (not a /nix/store path) so
  # host-mounted config.json can reference it. Empty dirs are mount points.
  extraCommands = ''
    mkdir -p emulatorjs config roms saves
    cp -a ${emulatorjs}/. emulatorjs/
    chmod 1777 config roms saves
  '';

  config = {
    Entrypoint = [ "${barp}/bin/barp" ];
    Cmd = [ "--config" "/config/config.json" ];
    ExposedPorts = {
      "3000/tcp" = { };
    };
    WorkingDir = "/";
    # Matches typical bind-mount ownership on the host; override with
    # `user:` in Compose if your UID differs.
    User = "1000:1000";
    Env = [
      "HOME=/tmp"
    ];
  };
}
