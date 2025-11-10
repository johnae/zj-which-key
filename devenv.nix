{pkgs, ...}: {
  env.GREET = "zj-which-key";
  packages = [pkgs.git pkgs.jujutsu];
  languages.rust = {
    enable = true;
    channel = "stable";
    targets = ["wasm32-wasip1"];
  };

  enterShell = ''
  '';
}
