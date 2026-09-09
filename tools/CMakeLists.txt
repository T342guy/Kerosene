# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# The toolset. Separate from the engine on purpose -- see the layering check in
# cmake/KeroseneTargets.cmake -- and each stage stays a subcommand so it can be
# scripted, run on a build server, or replaced.
