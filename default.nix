let
  pkgs = import <nixpkgs> {};
in

pkgs.rustPlatform.buildRustPackage rec {
  pname = "oxide";
  version = "0.9.8";

  src = pkgs.fetchFromGitHub {
    owner = "Tjens23";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-u7V91xYqYe/oPycEBtq3glBEdVjuLnlfTyOADPYktVw=";
  };

  cargoHash = "sha256-G+PTAzKiWsOofEBMOuLAR7e/q3Tt8dYAO1hDLxUl9dk=";

  meta = with pkgs.lib; {
    description = "npm package manager written in rust";
    homepage = "https://github.com/Tjens23/oxide/";
    license = licenses.mit;
    platforms = platforms.unix;
    maintainers = ["EnderSlain" "Tjens23"];
  };

}
