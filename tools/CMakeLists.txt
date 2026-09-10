# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# The toolset. Separate from the engine on purpose -- see the layering check in
# cmake/KeroseneTargets.cmake -- and each stage stays a subcommand so it can be
# scripted, run on a build server, or replaced.

add_subdirectory(common)
add_subdirectory(cleave)
add_subdirectory(umbra)

# One binary carrying every stage. The stages stay separate subcommands, so
# each is still scriptable and replaceable; what they share is the argument
# parsing, the content-tree lookup and the log, which is worth not writing
# eight times.
kerosene_executable(kerosene-tools
    SOURCES kerosene-tools.cpp
    DEPS kerosene::cleave kerosene::umbra kerosene::toolcommon)
