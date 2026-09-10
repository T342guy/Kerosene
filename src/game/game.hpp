// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "entity/entity.hpp"

/// The entity classes -- the game-DLL analogue.
///
/// Deliberately a separate library from the engine, and deliberately the only
/// one that knows what a `func_door` is. The engine loads a level, ticks a
/// world and draws it; what the things in the level *mean* lives here. That
/// boundary is why Source games could be modded without touching the engine,
/// and the build enforces it in the same direction the toolset boundary is
/// enforced: nothing below this library may reach up into it.
namespace kero::game {

/// The factory the engine hands to entity::World.
[[nodiscard]] entity::Factory factory();

/// Where the player starts. Falls back to a sensible spot when the level has
/// no `info_player_start`, because a level you cannot spawn in is harder to fix
/// than one you spawn in the wrong place in.
struct SpawnPoint {
    math::Vec3 origin;
    math::Angles angles;
    bool found = false;
};

[[nodiscard]] SpawnPoint find_spawn(entity::World& world);

}  // namespace kero::game
