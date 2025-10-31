{ getSystem, ... }: {
  flake.overlays.default = _: prev: {
    inherit ((getSystem prev.system).packages) shadow-harvester;
  };
}
