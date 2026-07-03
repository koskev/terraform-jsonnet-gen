{ inputs, ... }:

{
  imports = [ inputs.actions-nix.flakeModules.default ];
  flake.actions-nix = {
    pre-commit.enable = true;
    defaultValues = {
      jobs = {
        runs-on = "ubuntu-latest";
      };
    };
    workflows = {
      ".github/workflows/docker-publish.yaml" = inputs.nix-actions.lib.mkDocker { };
    };
  };
}
