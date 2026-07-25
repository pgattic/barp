{
  stdenv,
  fetchurl,
  p7zip,
}:

stdenv.mkDerivation rec {
  pname = "barp-emulatorjs";
  version = "4.2.3";

  src = fetchurl {
    url = "https://github.com/EmulatorJS/EmulatorJS/releases/download/v${version}/${version}.7z";
    hash = "sha256-B9RRvAb6OtBKsw2blOtjrDStC6vuUtYDV7ACvejzhQs=";
  };

  nativeBuildInputs = [ p7zip ];

  dontConfigure = true;
  dontBuild = true;

  unpackPhase = ''
    runHook preUnpack
    mkdir -p source
    7z x "$src" -osource
    runHook postUnpack
  '';

  installPhase = ''
    runHook preInstall
    data_dir="$(find source -type f -name loader.js -path '*/data/loader.js' -print -quit | sed 's#/loader.js$##')"
    if [ -z "$data_dir" ]; then
      echo "could not find data/loader.js in EmulatorJS release asset" >&2
      exit 1
    fi
    mkdir -p "$out"
    cp -a "$data_dir"/. "$out"/
    runHook postInstall
  '';

  meta = {
    description = "EmulatorJS prebuilt data assets for BARP";
    homepage = "https://github.com/EmulatorJS/EmulatorJS";
    # Upstream release asset used only for the browser runtime data tree.
  };
}
