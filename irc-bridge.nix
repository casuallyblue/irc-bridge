self: {config, lib, options, ...}: 
with lib;
let cfg = config.casuallyblue.services.irc-bridge; in {
  options = {
    casuallyblue.services.irc-bridge = {
      enable = mkEnableOption "casuallyblue.dev server";

      bridge-env-file = mkOption {
        type = types.str;
        description = "The age file to load env vars from";
      };
    };
  };

  config = mkIf cfg.enable {
    systemd.services."irc-bridge" = {
      wantedBy = ["multi-user.target"];

      serviceConfig = {
        User = "cbsite";
        Group = "users";
        Restart=  "on-failure";
        WorkingDirectory = "/tmp";
        RestartSec = "30s";
        Type = "simple";
      };

      script = let 
        bridge = self.packages.x86_64-linux.default;
      in ''
        source ${cfg.bridge-env-file}
        export BRIDGE_SQLITE_PATH=sqlite3:/var/lib/irc-bridge/bridge.sqlite3
        exec ${bridge}/bin/irc-bridge
      '';
    };
  };
}
