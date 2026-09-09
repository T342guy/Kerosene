// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "kv/keyvalues.hpp"

using namespace kero;
using namespace kero::kv;

namespace {

constexpr std::string_view kSample = R"KV(
versioninfo
{
	"editorversion" "100"
	"formatversion" "1"
}
world
{
	"id" "1"
	"classname" "worldspawn"
	solid
	{
		"id" "2"
		side
		{
			"id"       "3"
			"plane"    "(512 -8 0) (-8 -8 0) (-8 264 0)"
			"material" "dev/grid"
			"uaxis"    "[1 0 0 0] 0.25"
		}
		side
		{
			"id"       "4"
			"material" "dev/wall"
		}
	}
}
)KV";

}  // namespace

TEST_CASE("parses nested blocks and keeps them in file order") {
    auto document = parse(kSample, "kero_start.kmap");
    REQUIRE(document);

    REQUIRE(document->blocks.size() == 2);
    CHECK(document->blocks[0].name == "versioninfo");
    CHECK(document->blocks[1].name == "world");

    const Block* world = document->first("world");
    REQUIRE(world);
    CHECK(world->get("classname") == "worldspawn");

    const Block* solid = world->first_child("solid");
    REQUIRE(solid);

    // Repeated names are ordinary and must stay in order: brush sides are
    // referred to by position, so a parser that merged or reordered them would
    // corrupt maps rather than reject them.
    const std::vector<const Block*> sides = solid->children_named("side");
    REQUIRE(sides.size() == 2);
    CHECK(sides[0]->get("id") == "3");
    CHECK(sides[1]->get("id") == "4");
    CHECK(solid->count_children("side") == 2);
}

TEST_CASE("typed reads") {
    auto document = parse(R"KV(
block
{
	"count"   "42"
	"scale"   "0.25"
	"enabled" "1"
	"off"     "false"
	"origin"  "128 -64 32"
	"plane"   "(512 -8 0) (-8 -8 0) (-8 264 0)"
	"uaxis"   "[1 0 0 0] 0.25"
	"junk"    "12abc"
	"blank"   ""
}
)KV", "test");
    REQUIRE(document);
    const Block& block = document->blocks[0];

    CHECK(block.get_i32("count") == 42);
    CHECK(block.get_f32("scale") == doctest::Approx(0.25f));
    CHECK(block.get_bool("enabled") == true);
    CHECK(block.get_bool("off") == false);

    SUBCASE("a vector reads with or without brackets") {
        auto origin = block.get_vec3("origin");
        REQUIRE(origin);
        CHECK((*origin)[0] == doctest::Approx(128.0));
        CHECK((*origin)[2] == doctest::Approx(32.0));

        // The plane key holds three points; the first three numbers are the
        // first point, which is what a caller reading it one point at a time
        // expects.
        auto plane = block.get_vec3("plane");
        REQUIRE(plane);
        CHECK((*plane)[0] == doctest::Approx(512.0));
        CHECK((*plane)[1] == doctest::Approx(-8.0));

        auto uaxis = block.get_vec3("uaxis");
        REQUIRE(uaxis);
        CHECK((*uaxis)[0] == doctest::Approx(1.0));
    }

    SUBCASE("junk is rejected rather than read as a prefix") {
        // "12abc" must not quietly become 12.
        CHECK_FALSE(block.get_i32("junk"));
        CHECK_FALSE(block.get_f32("blank"));
        CHECK_FALSE(block.get_i32("missing"));
        CHECK_FALSE(block.get_vec3("count"));
    }

    SUBCASE("a missing key falls back without inventing a value") {
        CHECK(block.get("missing", "dev/grey") == "dev/grey");
        CHECK(block.get("missing").empty());
        CHECK_FALSE(block.has("missing"));
    }
}

TEST_CASE("diagnostics name the line and column") {
    SUBCASE("a block with no brace") {
        auto document = parse("world\n\"id\" \"1\"\n", "kero_start.kmap");
        REQUIRE_FALSE(document);
        CHECK(document.error().where.line == 2);
        CHECK(document.error().message.find("expected '{'") != std::string::npos);
        CHECK(document.error().format().starts_with("kero_start.kmap:2:"));
    }

    SUBCASE("a block that is never closed names where it opened") {
        // The useful location is where the block *started*, not the end of the
        // file -- the end of the file is where you already know something is
        // wrong.
        auto document = parse("world\n{\n\tsolid\n\t{\n\t\t\"id\" \"2\"\n", "m.kmap");
        REQUIRE_FALSE(document);
        CHECK(document.error().where.line == 3);
        CHECK(document.error().message.find("never closed") != std::string::npos);
    }

    SUBCASE("a key with no value") {
        auto document = parse("world\n{\n\t\"id\"\n}\n", "m.kmap");
        REQUIRE_FALSE(document);
        CHECK(document.error().where.line == 4);
        CHECK(document.error().message.find("no value") != std::string::npos);
    }

    SUBCASE("an unterminated string points at the opening quote") {
        auto document = parse("world\n{\n\t\"id\" \"1\n}\n", "m.kmap");
        REQUIRE_FALSE(document);
        CHECK(document.error().where.line == 3);
        CHECK(document.error().message.find("closing quote") != std::string::npos);
    }

    SUBCASE("a stray closing brace") {
        auto document = parse("}\n", "m.kmap");
        REQUIRE_FALSE(document);
        CHECK(document.error().message.find("unmatched") != std::string::npos);
    }

    SUBCASE("runaway nesting is reported, not crashed on") {
        std::string text;
        for (int i = 0; i < 5000; ++i) {
            text += "a\n{\n";
        }
        auto document = parse(text, "m.kmap");
        REQUIRE_FALSE(document);
        CHECK(document.error().message.find("nested") != std::string::npos);
    }
}

TEST_CASE("comments and whitespace are ignored") {
    auto document = parse(R"KV(
// A comment before everything.
world      // and one after a block name
{
	// and one inside
	"id" "1"  // and one after a pair
}
)KV", "test");
    REQUIRE(document);
    REQUIRE(document->blocks.size() == 1);
    CHECK(document->blocks[0].get("id") == "1");
}

TEST_CASE("escapes round-trip, and a lone backslash stays literal") {
    auto document = parse(R"KV(
block
{
	"quoted"  "say \"hello\""
	"newline" "a\nb"
	"path"    "materials\dev\grid"
}
)KV", "test");
    REQUIRE(document);
    const Block& block = document->blocks[0];

    CHECK(block.get("quoted") == "say \"hello\"");
    CHECK(block.get("newline") == "a\nb");
    // A backslash before an ordinary character is literal, so a path pasted
    // into a material key survives.
    CHECK(block.get("path") == "materials\\dev\\grid");

    auto again = parse(document->to_string(), "test");
    REQUIRE(again);
    CHECK(again->blocks[0].get("quoted") == "say \"hello\"");
    CHECK(again->blocks[0].get("path") == "materials\\dev\\grid");
}

TEST_CASE("a document round-trips through text without losing structure") {
    auto first = parse(kSample, "kero_start.kmap");
    REQUIRE(first);

    const std::string text = first->to_string();
    auto second = parse(text, "kero_start.kmap");
    REQUIRE(second);

    // Writing what was read and reading it back must give the same tree; that
    // is what makes it safe for the editor to save a map it opened.
    CHECK(second->to_string() == text);
    REQUIRE(second->blocks.size() == first->blocks.size());

    const Block* solid = second->first("world")->first_child("solid");
    REQUIRE(solid);
    REQUIRE(solid->count_children("side") == 2);
    CHECK(solid->children_named("side")[0]->get("plane") ==
          "(512 -8 0) (-8 -8 0) (-8 264 0)");
}

TEST_CASE("set replaces a value in place, or appends a new one") {
    auto document = parse("world\n{\n\t\"id\" \"1\"\n}\n", "test");
    REQUIRE(document);
    Block& world = document->blocks[0];

    world.set("id", "7");
    CHECK(world.get("id") == "7");
    CHECK(world.pairs.size() == 1);

    world.set("skyname", "sky_kero");
    CHECK(world.get("skyname") == "sky_kero");
    CHECK(world.pairs.size() == 2);
}

TEST_CASE("an empty document is valid, not an error") {
    auto document = parse("", "test");
    REQUIRE(document);
    CHECK(document->blocks.empty());

    auto comments_only = parse("// nothing here\n\n", "test");
    REQUIRE(comments_only);
    CHECK(comments_only->blocks.empty());
}

TEST_CASE("a file that will not open reports like a file that will not parse") {
    auto document = parse_file("/nonexistent/kero_start.kmap");
    REQUIRE_FALSE(document);
    CHECK(document.error().message.find("cannot open") != std::string::npos);
    CHECK(document.error().format().find("kero_start.kmap") != std::string::npos);
}
