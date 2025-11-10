{pkgs, ...}: {
  env.GREET = "zj-which-key";
  packages = with pkgs; [
    bashInteractive
    git
    jujutsu
    vhs
  ];
  languages.rust = {
    enable = true;
    channel = "stable";
    targets = ["wasm32-wasip1"];
  };

  enterShell = ''
  '';
}
