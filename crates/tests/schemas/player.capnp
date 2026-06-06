@0xdbb9ad1f14bf0b36;

struct Vector3 {
  x @0 :Float32;
  y @1 :Float32;
  z @2 :Float32;
  w @3 :Float32;
}

struct Player {
  playerId @0 :Float64;
  level    @1 :Float32;
  inventory @2 :List(Text);
  position @3 :Vector3;
  tags     @4 :List(Text);
  isNauseous @5 :Bool;
}