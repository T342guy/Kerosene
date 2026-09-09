# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# One warning set, defined once and applied by kerosene_library()/_executable().
# A warning that is worth turning on is worth turning on everywhere; a target
# that needs an exception should say so at the target, not by quietly using a
# different set.

add_library(kerosene_warnings INTERFACE)
add_library(kerosene::warnings ALIAS kerosene_warnings)

set(_kero_gnu_warnings
    -Wall
    -Wextra
    -Wpedantic
    # Conversions that lose information. Geometry code is exactly where an
    # implicit double->float will hide, so this one earns its noise.
    -Wconversion
    -Wsign-conversion
    -Wdouble-promotion
    -Wold-style-cast
    -Wcast-qual
    -Wcast-align
    -Wshadow
    -Wnon-virtual-dtor
    -Woverloaded-virtual
    -Wnull-dereference
    -Wformat=2
    -Wimplicit-fallthrough
    -Wmisleading-indentation
    -Wunused
    # Uninitialised reads in a BSP compiler are silent wrong answers, not
    # crashes, so they are worth the occasional false positive.
    -Wuninitialized
)

if(CMAKE_CXX_COMPILER_ID MATCHES "GNU")
    list(APPEND _kero_gnu_warnings
        -Wduplicated-cond
        -Wduplicated-branches
        -Wlogical-op)
    # Deliberately absent: -Wuseless-cast. It fires on casts that are redundant
    # on this platform but load-bearing on another -- size_t to uintptr_t is the
    # usual one -- so obeying it means writing code that is less portable, not
    # more correct.
endif()

if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    target_compile_options(kerosene_warnings INTERFACE ${_kero_gnu_warnings})
    if(KEROSENE_WERROR)
        target_compile_options(kerosene_warnings INTERFACE -Werror)
    endif()
elseif(MSVC)
    target_compile_options(kerosene_warnings INTERFACE /W4 /permissive-)
    if(KEROSENE_WERROR)
        target_compile_options(kerosene_warnings INTERFACE /WX)
    endif()
endif()
