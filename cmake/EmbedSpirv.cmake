# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Turns a .spv file into a header holding it as a byte array. Run as a script by
# Shaders.cmake, not included.

file(READ "${SPIRV}" hex HEX)
string(LENGTH "${hex}" length)
math(EXPR count "${length} / 2")

set(body "")
set(column 0)
foreach(index RANGE 1 ${count})
    math(EXPR offset "(${index} - 1) * 2")
    string(SUBSTRING "${hex}" ${offset} 2 byte)
    string(APPEND body "0x${byte},")
    math(EXPR column "${column} + 1")
    if(column EQUAL 16)
        string(APPEND body "\n    ")
        set(column 0)
    endif()
endforeach()

file(WRITE "${HEADER}"
"// Generated from ${SPIRV}. Do not edit.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

namespace kero::shaders {

inline constexpr std::array<std::uint8_t, ${count}> ${SYMBOL}{
    ${body}
};

}  // namespace kero::shaders
")
