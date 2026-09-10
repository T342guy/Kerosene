// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "math/aabb.hpp"
#include "math/angles.hpp"
#include "math/mat.hpp"
#include "math/plane.hpp"
#include "math/units.hpp"
#include "math/vec.hpp"
#include "math/winding.hpp"

#include <format>

using namespace kero;
using namespace kero::math;

namespace {

/// A unit cube's worth of brush sides, as .kmap would store them: six planes
/// whose normals face outward.
std::vector<Planed> box_planes(Vec3d mins, Vec3d maxs) {
    return {
        Planed(Vec3d(1, 0, 0), maxs.x),  Planed(Vec3d(-1, 0, 0), -mins.x),
        Planed(Vec3d(0, 1, 0), maxs.y),  Planed(Vec3d(0, -1, 0), -mins.y),
        Planed(Vec3d(0, 0, 1), maxs.z),  Planed(Vec3d(0, 0, -1), -mins.z),
    };
}

/// Clips a base winding by every plane except its own, which is how a brush
/// side becomes a face.
std::optional<Windingd> face_of(const std::vector<Planed>& planes, usize index) {
    std::optional<Windingd> winding = Windingd::from_plane(planes[index]);
    for (usize i = 0; i < planes.size() && winding; ++i) {
        if (i != index) {
            winding = winding->clipped(planes[i].flipped());
        }
    }
    return winding;
}

}  // namespace

TEST_CASE("units keep architectural sizes on the grid") {
    // The reason the scale is two inches and not something rounder: these are
    // the numbers a designer types, and they should be powers of two.
    CHECK(units::kPlayerHeight == 36.0f);
    CHECK(units::kPerFoot == 6.0f);
    CHECK(units::kPlayerWidth == 16.0f);
    CHECK(units::kWorldExtent == 8192.0f);

    // One ku is two inches, so a ku is 0.0508 m.
    CHECK(units::to_metres(1.0f) == doctest::Approx(0.0508).epsilon(0.001));
    CHECK(units::from_metres(units::to_metres(128.0f)) == doctest::Approx(128.0f));

    CHECK(units::format_distance_short(128.0f) == "128 ku");
    CHECK(units::format_distance_short(12.5f) == "12.5 ku");
    CHECK(units::format_size(512.0f, 384.0f, 128.0f) == "512 x 384 x 128");
    CHECK(units::format_in_players(72.0f) == "2 players");
}

TEST_CASE("vector basics") {
    const Vec3 a(3, 4, 0);
    CHECK(a.length() == doctest::Approx(5.0f));
    CHECK(dot(Vec3::unit_x(), Vec3::unit_y()) == 0.0f);
    CHECK(cross(Vec3::unit_x(), Vec3::unit_y()) == Vec3::unit_z());

    CHECK(Vec3(1, 5, 2).major_axis() == 1);
    CHECK(Vec3(-9, 5, 2).major_axis() == 0);
    CHECK(Vec3(1, 5, -7).major_axis() == 2);

    SUBCASE("a perpendicular exists for every axis, including its own") {
        for (const Vec3& v : {Vec3::unit_x(), Vec3::unit_y(), Vec3::unit_z(),
                              Vec3(1, 1, 1).normalized()}) {
            const Vec3 perpendicular = any_perpendicular(v);
            CHECK(perpendicular.length() == doctest::Approx(1.0f));
            CHECK(dot(perpendicular, v) == doctest::Approx(0.0f).epsilon(1e-5));
        }
    }

    SUBCASE("normalising a zero vector reports failure rather than producing NaN") {
        Vec3 zero(0, 0, 0);
        CHECK(zero.normalize() == 0.0f);
        CHECK(zero == Vec3(0, 0, 0));
    }

    SUBCASE("formats the way .kmap writes coordinates") {
        CHECK(std::format("{}", Vec3(128, -64, 32)) == "(128 -64 32)");
    }
}

TEST_CASE("plane from three points") {
    Planed plane;

    SUBCASE("a counter-clockwise triple faces the viewer") {
        REQUIRE(Planed::from_points(Vec3d(0, 0, 0), Vec3d(0, 16, 0), Vec3d(16, 0, 0), plane));
        CHECK(plane.normal.z == doctest::Approx(1.0));
        CHECK(plane.distance == doctest::Approx(0.0));
        CHECK(plane.type == PlaneType::Z);
    }

    SUBCASE("collinear points are not a plane") {
        CHECK_FALSE(Planed::from_points(Vec3d(0, 0, 0), Vec3d(8, 0, 0), Vec3d(16, 0, 0), plane));
    }

    SUBCASE("coincident points are not a plane") {
        CHECK_FALSE(Planed::from_points(Vec3d(4, 4, 4), Vec3d(4, 4, 4), Vec3d(9, 2, 1), plane));
    }

    SUBCASE("a nearly-collinear triple still yields a usable plane in double precision") {
        // The kind of sliver that single-precision CSG turns into a leak.
        REQUIRE(Planed::from_points(Vec3d(0, 0, 0), Vec3d(1024, 0, 0), Vec3d(512, 0.01, 0), plane));
        CHECK(std::abs(plane.normal.z) == doctest::Approx(1.0));
    }
}

TEST_CASE("plane snapping puts grid-aligned walls back on their axis") {
    // What a plane looks like after being derived from three points that were
    // themselves the result of arithmetic.
    Planed plane(Vec3d(0.9999999, 0.0000004, 0.0), 127.99999998);
    plane.snap();

    CHECK(plane.normal == Vec3d(1, 0, 0));
    CHECK(plane.distance == doctest::Approx(128.0));
    CHECK(plane.type == PlaneType::X);
    CHECK(is_axial(plane.type));
}

TEST_CASE("plane equivalence distinguishes facing from position") {
    const Planed wall(Vec3d(1, 0, 0), 128.0);
    CHECK(wall.equivalent(Planed(Vec3d(1, 0, 0), 128.0)));
    CHECK_FALSE(wall.equivalent(Planed(Vec3d(1, 0, 0), 132.0)));
    CHECK_FALSE(wall.equivalent(wall.flipped()));
    CHECK(wall.coplanar(wall.flipped()));
}

TEST_CASE("point classification honours the tolerance, not the sign bit") {
    const Planed floor(Vec3d(0, 0, 1), 0.0);
    CHECK(floor.classify(Vec3d(0, 0, 16)) == Side::Front);
    CHECK(floor.classify(Vec3d(0, 0, -16)) == Side::Back);
    CHECK(floor.classify(Vec3d(0, 0, 0)) == Side::On);
    // Inside the tolerance: this is a vertex that arithmetic moved, not a
    // vertex that is on the other side of the wall.
    CHECK(floor.classify(Vec3d(0, 0, 0.001)) == Side::On);
    CHECK(floor.classify(Vec3d(0, 0, 0.5)) == Side::Front);
}

TEST_CASE("a base winding covers the plane and lies on it") {
    const Planed plane(Vec3d(0, 0, 1), 64.0);
    const Windingd winding = Windingd::from_plane(plane);

    REQUIRE(winding.size() == 4);
    CHECK(winding.valid());
    for (const Vec3d& point : winding.points()) {
        CHECK(plane.distance_to(point) == doctest::Approx(0.0).epsilon(1e-9));
    }

    Planed derived;
    REQUIRE(winding.plane(derived));
    CHECK(derived.equivalent(plane));

    // Large enough to survive being clipped down by anything inside the world.
    const f64 world = static_cast<f64>(units::kWorldExtent);
    CHECK(winding.area() > 4.0 * world * world);
}

TEST_CASE("splitting a winding") {
    // A 128 x 128 square on the floor plane, centred on the origin.
    const Windingd square(std::vector<Vec3d>{
        Vec3d(-64, -64, 0), Vec3d(64, -64, 0), Vec3d(64, 64, 0), Vec3d(-64, 64, 0)});
    REQUIRE(square.area() == doctest::Approx(128.0 * 128.0));

    std::optional<Windingd> front;
    std::optional<Windingd> back;

    SUBCASE("a plane through the middle halves the area, losing none of it") {
        square.split(Planed(Vec3d(1, 0, 0), 0.0), front, back);
        REQUIRE(front);
        REQUIRE(back);
        CHECK(front->area() == doctest::Approx(64.0 * 128.0));
        CHECK(back->area() == doctest::Approx(64.0 * 128.0));
        // The property that matters: splitting conserves area exactly. A crack
        // between the halves would show up here as a shortfall.
        CHECK(front->area() + back->area() == doctest::Approx(square.area()));
    }

    SUBCASE("an off-centre split still conserves area") {
        square.split(Planed(Vec3d(0, 1, 0), 33.0), front, back);
        REQUIRE(front);
        REQUIRE(back);
        CHECK(front->area() + back->area() == doctest::Approx(square.area()));
    }

    SUBCASE("a diagonal split conserves area") {
        Planed diagonal(Vec3d(1, 1, 0).normalized(), 0.0);
        square.split(diagonal, front, back);
        REQUIRE(front);
        REQUIRE(back);
        CHECK(front->area() + back->area() == doctest::Approx(square.area()));
    }

    SUBCASE("entirely in front") {
        square.split(Planed(Vec3d(0, 0, 1), -16.0), front, back);
        CHECK(front);
        CHECK_FALSE(back);
    }

    SUBCASE("entirely behind") {
        square.split(Planed(Vec3d(0, 0, 1), 16.0), front, back);
        CHECK_FALSE(front);
        CHECK(back);
    }

    SUBCASE("coplanar belongs to neither side") {
        square.split(Planed(Vec3d(0, 0, 1), 0.0), front, back);
        CHECK_FALSE(front);
        CHECK_FALSE(back);
    }

    SUBCASE("a tangent split produces nothing degenerate") {
        // Exactly along the edge: the sliver on one side has no area and must
        // be discarded rather than emitted as a face.
        square.split(Planed(Vec3d(1, 0, 0), 64.0), front, back);
        CHECK_FALSE(front);
        REQUIRE(back);
        CHECK(back->area() == doctest::Approx(square.area()));
    }

    SUBCASE("an axial split pins the shared coordinate exactly") {
        square.split(Planed(Vec3d(1, 0, 0), 17.0), front, back);
        REQUIRE(front);
        // The two vertices the split created sit exactly on the plane -- 17,
        // not 16.999999999998. That is what keeps the two halves welded along
        // the cut instead of leaving a hairline crack in the wall.
        usize on_plane = 0;
        for (const Vec3d& point : front->points()) {
            if (point.x != 64.0) {
                CHECK(point.x == 17.0);
                ++on_plane;
            }
        }
        CHECK(on_plane == 2);
    }

    SUBCASE("the split vertices are shared bit-for-bit between the halves") {
        square.split(Planed(Vec3d(1, 2, 0).normalized(), 5.0), front, back);
        REQUIRE(front);
        REQUIRE(back);
        usize shared = 0;
        for (const Vec3d& f : front->points()) {
            for (const Vec3d& b : back->points()) {
                if (f == b) {
                    ++shared;
                }
            }
        }
        CHECK(shared == 2);
    }
}

TEST_CASE("clipping a base winding by six planes produces a cube's faces") {
    const std::vector<Planed> planes = box_planes(Vec3d(0, 0, 0), Vec3d(128, 128, 128));

    f64 total_area = 0.0;
    for (usize i = 0; i < planes.size(); ++i) {
        const std::optional<Windingd> face = face_of(planes, i);
        REQUIRE_MESSAGE(face, "face " << i << " of a well-formed cube vanished");
        CHECK(face->size() == 4);
        CHECK(face->area() == doctest::Approx(128.0 * 128.0));
        total_area += face->area();

        Planed derived;
        REQUIRE(face->plane(derived));
        CHECK(derived.equivalent(planes[i]));
    }
    CHECK(total_area == doctest::Approx(6.0 * 128.0 * 128.0));
}

TEST_CASE("a brush with no interior produces no faces") {
    // Opposite walls swapped: the half-spaces do not intersect. Hand-edited
    // maps contain these, so the answer has to be "nothing", not a crash.
    std::vector<Planed> planes = box_planes(Vec3d(0, 0, 0), Vec3d(128, 128, 128));
    planes[0].distance = -16.0;  // +X face pulled behind the -X face.

    usize surviving = 0;
    for (usize i = 0; i < planes.size(); ++i) {
        if (face_of(planes, i)) {
            ++surviving;
        }
    }
    CHECK(surviving == 0);
}

TEST_CASE("collinear points are removed but the shape is not") {
    // The middle point of the bottom edge carries no shape -- exactly what a
    // split leaves behind when the neighbouring fragment is clipped away.
    Windingd winding(std::vector<Vec3d>{
        Vec3d(-64, -64, 0), Vec3d(0, -64, 0), Vec3d(64, -64, 0),
        Vec3d(64, 64, 0), Vec3d(-64, 64, 0)});
    const f64 before = winding.area();

    winding.remove_collinear();

    CHECK(winding.size() == 4);
    CHECK(winding.area() == doctest::Approx(before));
}

TEST_CASE("degenerate windings are rejected") {
    CHECK_FALSE(Windingd(std::vector<Vec3d>{Vec3d(0, 0, 0), Vec3d(1, 0, 0)}).valid());

    // Zero area: three collinear points.
    CHECK_FALSE(Windingd(std::vector<Vec3d>{
        Vec3d(0, 0, 0), Vec3d(64, 0, 0), Vec3d(128, 0, 0)}).valid());

    // A sliver: long, but thinner than anything that can be textured or lit.
    CHECK_FALSE(Windingd(std::vector<Vec3d>{
        Vec3d(0, 0, 0), Vec3d(1024, 0, 0), Vec3d(1024, 0.0001, 0)}).valid());

    CHECK(Windingd(std::vector<Vec3d>{
        Vec3d(0, 0, 0), Vec3d(16, 0, 0), Vec3d(16, 16, 0)}).valid());
}

TEST_CASE("snapping welds coordinates that arithmetic moved off the grid") {
    Windingd winding(std::vector<Vec3d>{
        Vec3d(-63.999999998, -64.000000001, 0.0000000004),
        Vec3d(64.000000002, -64.0, 0),
        Vec3d(64.0, 63.999999999, 0),
        Vec3d(-64.0, 64.0, 0)});

    winding.snap();

    REQUIRE(winding.size() == 4);
    CHECK(winding[0] == Vec3d(-64, -64, 0));
    CHECK(winding[1] == Vec3d(64, -64, 0));
    CHECK(winding.area() == doctest::Approx(128.0 * 128.0));
}

TEST_CASE("winding classification against a plane") {
    const Windingd square(std::vector<Vec3d>{
        Vec3d(-64, -64, 0), Vec3d(64, -64, 0), Vec3d(64, 64, 0), Vec3d(-64, 64, 0)});

    CHECK(square.classify(Planed(Vec3d(0, 0, 1), -16.0)) == Side::Front);
    CHECK(square.classify(Planed(Vec3d(0, 0, 1), 16.0)) == Side::Back);
    CHECK(square.classify(Planed(Vec3d(0, 0, 1), 0.0)) == Side::On);
    CHECK(square.classify(Planed(Vec3d(1, 0, 0), 0.0)) == Side::Crossing);
}

TEST_CASE("windings survive the far corner of the world") {
    // Where single-precision CSG starts producing slivers. The compile-time
    // instantiation is double, which is the entire point.
    const f64 far_edge = static_cast<f64>(units::kWorldExtent) - 128.0;
    const std::vector<Planed> planes =
        box_planes(Vec3d(far_edge, far_edge, far_edge),
                   Vec3d(far_edge + 128.0, far_edge + 128.0, far_edge + 128.0));

    for (usize i = 0; i < planes.size(); ++i) {
        const std::optional<Windingd> face = face_of(planes, i);
        REQUIRE(face);
        CHECK(face->area() == doctest::Approx(128.0 * 128.0));
    }
}

TEST_CASE("aabb") {
    Aabb box;
    CHECK(box.empty());

    box.add(Vec3(0, 0, 0));
    box.add(Vec3(128, 64, 32));
    CHECK_FALSE(box.empty());
    CHECK(box.size() == Vec3(128, 64, 32));
    CHECK(box.centre() == Vec3(64, 32, 16));
    CHECK(box.contains(Vec3(64, 32, 16)));
    CHECK_FALSE(box.contains(Vec3(200, 32, 16)));

    SUBCASE("touching boxes intersect, because a broadphase must consider them") {
        const Aabb neighbour(Vec3(128, 0, 0), Vec3(256, 64, 32));
        CHECK(box.intersects(neighbour));
        CHECK_FALSE(box.intersects(Aabb(Vec3(129, 0, 0), Vec3(256, 64, 32))));
    }

    SUBCASE("support picks the corner furthest along a direction") {
        CHECK(box.support(Vec3(1, 1, 1)) == Vec3(128, 64, 32));
        CHECK(box.support(Vec3(-1, -1, -1)) == Vec3(0, 0, 0));
        CHECK(box.support(Vec3(1, -1, 1)) == Vec3(128, 0, 32));
    }
}

TEST_CASE("pitch is positive downward, and stays that way") {
    Vec3 forward;

    angle_vectors(Angles(0, 0, 0), &forward, nullptr, nullptr);
    CHECK(forward.x == doctest::Approx(1.0f));
    CHECK(forward.z == doctest::Approx(0.0f));

    // Looking at the floor.
    angle_vectors(Angles(90, 0, 0), &forward, nullptr, nullptr);
    CHECK(forward.z == doctest::Approx(-1.0f));

    // Looking at the sky.
    angle_vectors(Angles(-90, 0, 0), &forward, nullptr, nullptr);
    CHECK(forward.z == doctest::Approx(1.0f));

    // Yaw 90 faces +Y.
    angle_vectors(Angles(0, 90, 0), &forward, nullptr, nullptr);
    CHECK(forward.y == doctest::Approx(1.0f));
}

TEST_CASE("angle vectors form an orthonormal basis") {
    for (const Angles& angles : {Angles(0, 0, 0), Angles(30, 45, 15), Angles(-60, 200, -90)}) {
        Vec3 forward;
        Vec3 right;
        Vec3 up;
        angle_vectors(angles, &forward, &right, &up);

        CHECK(forward.length() == doctest::Approx(1.0f));
        CHECK(right.length() == doctest::Approx(1.0f));
        CHECK(up.length() == doctest::Approx(1.0f));
        CHECK(dot(forward, right) == doctest::Approx(0.0f).epsilon(1e-5));
        CHECK(dot(forward, up) == doctest::Approx(0.0f).epsilon(1e-5));
        CHECK(dot(right, up) == doctest::Approx(0.0f).epsilon(1e-5));
    }
}

TEST_CASE("a direction round-trips through angles") {
    for (const Angles& original : {Angles(0, 0, 0), Angles(30, 45, 0), Angles(-75, 190, 0)}) {
        Vec3 forward;
        angle_vectors(original, &forward, nullptr, nullptr);
        const Angles recovered = vector_to_angles(forward);

        Vec3 again;
        angle_vectors(recovered, &again, nullptr, nullptr);
        CHECK(again.x == doctest::Approx(forward.x).epsilon(1e-4));
        CHECK(again.y == doctest::Approx(forward.y).epsilon(1e-4));
        CHECK(again.z == doctest::Approx(forward.z).epsilon(1e-4));
    }
}

TEST_CASE("angle normalisation takes the short way round") {
    CHECK(normalize_angle(370.0f) == doctest::Approx(10.0f));
    CHECK(normalize_angle(-370.0f) == doctest::Approx(-10.0f));
    CHECK(normalize_angle(180.0f) == doctest::Approx(-180.0f));
    CHECK(angle_difference(350.0f, 10.0f) == doctest::Approx(20.0f));
    CHECK(angle_difference(10.0f, 350.0f) == doctest::Approx(-20.0f));
}


TEST_CASE("matrix multiplication and identity") {
    const Mat4 identity = Mat4::identity();
    const Mat4 projection = Mat4::perspective(75.0f, 16.0f / 9.0f, 1.0f, 4096.0f);

    const Mat4 product = projection * identity;
    for (usize column = 0; column < 4; ++column) {
        for (usize row = 0; row < 4; ++row) {
            CHECK(product.m[column][row] == doctest::Approx(projection.m[column][row]));
        }
    }

    const Vec4 point = identity * Vec4(1, 2, 3, 1);
    CHECK(point.x == doctest::Approx(1.0f));
    CHECK(point.z == doctest::Approx(3.0f));
}

TEST_CASE("the projection maps the view volume onto depth 0 to 1") {
    const f32 near = 1.0f;
    const f32 far = 1024.0f;
    const Mat4 projection = Mat4::perspective(90.0f, 1.0f, near, far);

    // A right-handed view space: the camera looks down -Z.
    const Vec4 at_near = projection * Vec4(0, 0, -near, 1);
    const Vec4 at_far = projection * Vec4(0, 0, -far, 1);

    REQUIRE(at_near.w > 0.0f);
    REQUIRE(at_far.w > 0.0f);
    // Zero at the near plane and one at the far plane, which is what Vulkan,
    // D3D12 and Metal all expect.
    CHECK(at_near.z / at_near.w == doctest::Approx(0.0f).epsilon(1e-4));
    CHECK(at_far.z / at_far.w == doctest::Approx(1.0f).epsilon(1e-4));

    // At 90 degrees and square aspect, the edge of the volume is at 45 degrees.
    const Vec4 edge = projection * Vec4(10, 0, -10, 1);
    CHECK(edge.x / edge.w == doctest::Approx(1.0f).epsilon(1e-4));
}

TEST_CASE("the view matrix puts the world in front of the camera") {
    // Standing at the origin looking east along +X, which is yaw 0.
    const Mat4 view = Mat4::view_from_angles(Vec3(0, 0, 0), Angles(0, 0, 0));

    // A point 100 ku ahead should land 100 ku down the camera's -Z.
    const Vec4 ahead = view * Vec4(100, 0, 0, 1);
    CHECK(ahead.z == doctest::Approx(-100.0f));
    CHECK(ahead.x == doctest::Approx(0.0f).epsilon(1e-4));
    CHECK(ahead.y == doctest::Approx(0.0f).epsilon(1e-4));

    // A point behind lands on +Z.
    CHECK((view * Vec4(-100, 0, 0, 1)).z == doctest::Approx(100.0f));

    SUBCASE("up in the world is up on the screen") {
        const Vec4 above = view * Vec4(100, 0, 50, 1);
        CHECK(above.y == doctest::Approx(50.0f));
    }

    SUBCASE("moving the mouse right turns towards the world's right") {
        // Yaw 0 faces +X; the camera's right is then -Y.
        const Vec4 to_the_right = view * Vec4(100, -50, 0, 1);
        CHECK(to_the_right.x == doctest::Approx(50.0f));
    }

    SUBCASE("the eye position is subtracted") {
        const Mat4 moved = Mat4::view_from_angles(Vec3(100, 0, 0), Angles(0, 0, 0));
        const Vec4 at_the_eye = moved * Vec4(100, 0, 0, 1);
        CHECK(at_the_eye.x == doctest::Approx(0.0f).epsilon(1e-4));
        CHECK(at_the_eye.z == doctest::Approx(0.0f).epsilon(1e-4));
    }
}

TEST_CASE("the frustum keeps what is in view and discards what is not") {
    const Mat4 projection = Mat4::perspective(75.0f, 16.0f / 9.0f, 1.0f, 4096.0f);
    const Mat4 view = Mat4::view_from_angles(Vec3(0, 0, 0), Angles(0, 0, 0));
    const Frustum frustum = Frustum::from_view_projection(projection * view);

    SUBCASE("a box straight ahead is visible") {
        CHECK(frustum.intersects(Aabb(Vec3(200, -32, -32), Vec3(264, 32, 32))));
    }

    SUBCASE("a box behind the camera is not") {
        CHECK_FALSE(frustum.intersects(Aabb(Vec3(-264, -32, -32), Vec3(-200, 32, 32))));
    }

    SUBCASE("a box far off to the side is not") {
        CHECK_FALSE(frustum.intersects(Aabb(Vec3(200, 2000, -32), Vec3(264, 2064, 32))));
    }

    SUBCASE("a box beyond the far plane is not") {
        CHECK_FALSE(frustum.intersects(Aabb(Vec3(8000, -32, -32), Vec3(8064, 32, 32))));
    }

    SUBCASE("a box straddling the edge is kept") {
        // Conservative on purpose: drawing something invisible costs
        // microseconds, and not drawing something visible is a hole in the
        // world.
        CHECK(frustum.intersects(Aabb(Vec3(200, -4000, -32), Vec3(264, 0, 32))));
    }

    SUBCASE("a box containing the camera is visible") {
        CHECK(frustum.intersects(Aabb(Vec3(-128, -128, -128), Vec3(128, 128, 128))));
    }
}

TEST_CASE("turning the camera changes what the frustum keeps") {
    const Mat4 projection = Mat4::perspective(75.0f, 16.0f / 9.0f, 1.0f, 4096.0f);
    const Aabb east(Vec3(200, -32, -32), Vec3(264, 32, 32));

    const Frustum looking_east = Frustum::from_view_projection(
        projection * Mat4::view_from_angles(Vec3(0, 0, 0), Angles(0, 0, 0)));
    const Frustum looking_west = Frustum::from_view_projection(
        projection * Mat4::view_from_angles(Vec3(0, 0, 0), Angles(0, 180, 0)));

    CHECK(looking_east.intersects(east));
    CHECK_FALSE(looking_west.intersects(east));
}
