@0xd4e3f2a1b8c97650;

enum Status {
  alive        @0;
  dead         @1;
  downed       @2;
  # renamed from dying
  disqualified @3;
  spectating   @4;
}

enum Class {
  warrior     @0;
  mage        @1;
  assassin    @2;  # renamed from rogue
  necromancer @3;
  paladin     @4;
}

enum Region {
  north   @0;
  south   @1;
  east    @2;
  west    @3;
  central @4;
  void    @5;
}

struct Vector3 {
  x @0 :Float32;
  y @1 :Float32;
  z @2 :Float32;
  w @3 :Float32;
}

struct AABB {
  minX @0 :Float32;
  minY @1 :Float32;
  maxX @2 :Float32;
  maxY @3 :Float32;
}

struct Transform {
  position @0 :Vector3;
  bounds   @1 :AABB;
}

struct Velocity {
  x @0 :Float32;
  y @1 :Float32;
  z @2 :Float32;
}

struct Stats {
  strength     @0 :Int32;
  agility      @1 :Int32;
  intelligence @2 :Int32;
  endurance    @3 :Int32;
  charisma     @4 :Int32 = 0;
  resistance   @5 :Float32 = 0.0;
}

struct Coordinates {
  x @0 :Float32;
  y @1 :Float32;
}

struct KillDeath {
  kills  @0 :Int32;
  deaths @1 :Int32;
}

struct Point3D {
  x @0 :Float32;
  y @1 :Float32;
  z @2 :Float32;
}

struct QuestProgressEntry {
  key   @0 :Text;
  value @1 :Int32;
}

struct QuickSlotsEntry {
  key   @0 :Int32;
  value @1 :Text;
}

struct SkillLevelsEntry {
  key   @0 :Text;
  value @1 :Float32;
}

struct LoadoutEntry {
  item     @0 :Text;
  quantity @1 :Int32;
}

struct LeaderboardScoresEntry {
  key   @0 :Text;
  value @1 :Int64;
}

struct Player {
  playerId          @0  :Float64;
  level             @1  :Float32;
  status            @2  :Status = alive;
  class             @3  :Class  = warrior;
  inventory         @4  :List(Text);
  callsign          @5  :Text;            # string(6)  — no fixed-len in capnp
  position          @6  :Vector3;
  tags              @7  :List(Text);
  transform         @8  :Transform;
  velocity          @9  :Velocity;
  statusCode        @10 :Text;
  # string(8)  — no fixed-len in capnp
  isNauseous        @11 :Bool;

  region            @12 :Region = north;
  stats             @13 :Stats;
  hotbar            @14 :List(Text);      # string(6)  — no fixed-len in capnp
  sessionToken      @15 :Text;            # string(16) — no fixed-len in capnp
  coordinates       @16 :Coordinates;
  killDeath         @17 :KillDeath;
  recentZones       @18 :List(Text);      # string(8)  — no fixed-len in capnp
  chatHistory       @19 :List(Text);      # string(32) — no fixed-len in capnp
  guildTag          @20 :Text;            # string(4)  — no fixed-len in capnp
  spawnPoint        @21 :Point3D;
  achievementIds    @22 :List(UInt32);
  activePerks       @23 :UInt32 = 0;      # bitset Perks
  accountFlags      @24 :UInt32 = 0;      # bitset Flags
  questProgress     @25 :List(QuestProgressEntry);
  quickSlots        @26 :List(QuickSlotsEntry); # map(10) limit handling
  skillLevels       @27 :List(SkillLevelsEntry);
  loadout           @28 :List(LoadoutEntry);    # loadout(4)
  leaderboardScores @29 :List(LeaderboardScoresEntry);
  partyMembers      @30 :List(UInt32);    # u32(4)
  lastPosition      @31 :Point3D;
}