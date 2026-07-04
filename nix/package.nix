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
      naersk' = pkgs.callPackage inputs.naersk { };

      rustPackage = naersk'.buildPackage rec {
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
