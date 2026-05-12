let
  pkgs = import <nixpkgs> {};
in

pkgs.rustPlatform.buildRustPackage rec {
  pname = "oxide";
  version = "0.9.7";

  src = pkgs.fetchFromGitHub {
    owner = "Tjens23";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-rT+l1L5HyK0QgmQdYgcNDyPQzASFWqEAu1jSPS8NS64=";
  };

  cargoHash = "sha256-ibZDkKUAiH4//fEVc8k42R+0ZW4aj4lEqaAEg78JrJo=";

  meta = with pkgs.lib; {
    description = "npm package manager written in rust";
    homepage = "https://github.com/Tjens23/oxide/";
    license = licenses.mit;
    platforms = platforms.unix;
    maintainers = ["EnderSlain" "Tjens23"];
  };

}
