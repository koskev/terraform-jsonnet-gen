{ inputs, ... }:
let
  inherit (inputs.nix-actions.lib) steps;
in

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
      ".github/workflows/test.yaml" = {
        name = "Test gernerating kubernetes";
        on = {
          push = { };
        };
        jobs = {
          build = {
            steps = [
              steps.checkout
              steps.installNix
              {
                name = "Test";
                run = "nix develop . --command make test";
              }
            ];
          };
        };
      };
      ".github/workflows/docker-publish.yaml" = inputs.nix-actions.lib.mkDocker {
        onConfig = {
          push = { };
        };
      };
    };
  };
}
