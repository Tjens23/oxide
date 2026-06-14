let
  pkgs = import <nixpkgs> { };
in

pkgs.rustPlatform.buildRustPackage rec {
  pname = "oxide";
  version = "0.13.0";

  src = pkgs.fetchFromGitHub {
    owner = "Tjens23";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-8Rl16on6pPc93J9wbWFfgmwXLSm8DB5amVeVx337veE=";
  };

  cargoHash = "sha256-UYzCJI+KIiVXoeg+9k79YRcUfDynWjGbNAGIcJdx6yg=";
  meta = with pkgs.lib; {
    description = "npm package manager written in rust";
    homepage = "https://github.com/Tjens23/oxide/";
    license = licenses.mit;
    platforms = platforms.unix;
    maintainers = [ "EnderSlain" "Tjens23" ];
  };

}