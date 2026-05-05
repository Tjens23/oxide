let
  pkgs = import <nixpkgs> {};
in

pkgs.rustPlatform.buildRustPackage rec {
  pname = "oxide";
  version = "0.3.0";

  src = pkgs.fetchFromGitHub {
    owner = "Tjens23";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-thYcZVE76AAJwJx24K+xV6HZ7oZwsi9NszdRcW/h0xk=";
  };

  cargoHash = "sha256-rT4ozLK1nGdKCytW+WT1zdKq4MVlCsLoTnmNRfRcElg=";

  meta = with pkgs.lib; {
    description = "npm package manager written in rust";
    homepage = "https://github.com/Tjens23/oxide/";
    license = licenses.mit;
    platforms = platforms.unix;
    maintainers = ["EnderSlain" "Tjens23"];
  };

}
