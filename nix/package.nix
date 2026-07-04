{
  inputs,
  self,
  ...
}:
{
  perSystem =
    {
      pkgs,
      lib,
      ...
    }:
    let
      craneLib = inputs.crane.mkLib pkgs;

      rustPackage = craneLib.buildPackage rec {
        name = "terraform-jsonnet-gen";
        meta.mainProgram = name;
        src = self;
      };

    in
    {
      packages = {
        default = pkgs.writeShellApplication {
          name = "terraform-jsonnet-gen";
          runtimeInputs = with pkgs; [
            rustPackage
            opentofu
          ];
          text = ''
            ${lib.getExe rustPackage} "$@"
          '';
        };
      };
    };
}
