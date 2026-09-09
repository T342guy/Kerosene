// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "math/units.hpp"

#include <cmath>
#include <format>

namespace kero::units {
namespace {

/// Level geometry is nearly all integers, and "128.00" is noise where "128" is
/// a measurement. Trailing zeroes go; genuine fractions stay.
std::string trim_number(f32 value) {
    std::string text = std::format("{:.2f}", value);
    if (text.find('.') != std::string::npos) {
        while (!text.empty() && text.back() == '0') {
            text.pop_back();
        }
        if (!text.empty() && text.back() == '.') {
            text.pop_back();
        }
    }
    return text;
}

}  // namespace

std::string format_distance(f32 ku) {
    return std::format("{} ku ({:.2f} m)", trim_number(ku), static_cast<f64>(to_metres(ku)));
}

std::string format_distance_short(f32 ku) {
    return std::format("{} ku", trim_number(ku));
}

std::string format_size(f32 x, f32 y, f32 z) {
    return std::format("{} x {} x {}", trim_number(x), trim_number(y), trim_number(z));
}

std::string format_in_players(f32 ku) {
    const f32 players = ku / kPlayerHeight;
    if (players < 1.0f) {
        return std::format("{:.0f}% of a player", static_cast<f64>(players * 100.0f));
    }
    return std::format("{} players", trim_number(players));
}

}  // namespace kero::units
