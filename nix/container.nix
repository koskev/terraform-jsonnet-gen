_: {
  perSystem =
    {
      inputs',
      self',
      lib,
      ...
    }:
    let
      nix2containerPkgs = inputs'.nix2container.packages;
    in
    {
      packages = {
        dockerImageFull = nix2containerPkgs.nix2container.buildImage {
          name = "terraform-jsonnet-gen";
          tag = "latest";

          config = {
            Entrypoint = [
              (lib.getExe self'.packages.default)
            ];
            Env = [
              "PATH=${self'.packages.default}/bin"
            ];
          };
        };
      };
    };
}
