{
  dockerTools,
  barp,
  emulatorjs,
}:

dockerTools.buildLayeredImage {
  name = "ghcr.io/pgattic/barp";
  tag = barp.version;

  contents = [ barp ];

  # EmulatorJS at a stable /emulatorjs path (not a /nix/store path) so
  # host-mounted config.json can reference it. Empty dirs are mount points.
  extraCommands = ''
    mkdir -p emulatorjs config roms saves
    cp -a ${emulatorjs}/. emulatorjs/
    chmod 0777 saves
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
  };
}
