let
  pkgs = import <nixpkgs> {};
in

pkgs.rustPlatform.buildRustPackage rec {
  pname = "oxide";
  version = "0.8.1";

  src = pkgs.fetchFromGitHub {
    owner = "Tjens23";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-RQkgGJwyO+qel/17An1I8ggf458Zg62MnQW9iTlCwX8=";
  };

  cargoHash = "sha256-M9v7Mr+8RAIKcwTSLRJQEwDCa7Y5toJ8OisaU1/lQ38=";

  meta = with pkgs.lib; {
    description = "npm package manager written in rust";
    homepage = "https://github.com/Tjens23/oxide/";
    license = licenses.mit;
    platforms = platforms.unix;
    maintainers = ["EnderSlain" "Tjens23"];
  };

}
