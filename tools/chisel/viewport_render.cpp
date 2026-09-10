// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/viewport_render.hpp"

#include "asset/devtexture.hpp"
#include "core/log.hpp"
#include "math/units.hpp"

#include <SDL3/SDL.h>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <format>
#include <unordered_map>

#include "chisel_line_vert.hpp"
#include "chisel_line_frag.hpp"
#include "chisel_solid_vert.hpp"
#include "chisel_solid_frag.hpp"

namespace kero::chisel {
namespace {

KERO_LOG_CATEGORY(log, "chisel");

struct LineVertex {
    f32 position[3];
    u8 colour[4];
};
static_assert(sizeof(LineVertex) == 16);

struct SolidVertex {
    f32 position[3];
    f32 uv[2];
    f32 normal[3];
};
static_assert(sizeof(SolidVertex) == 32);

struct CameraUniform {
    f32 view_projection[16];
    f32 tint[4];
};

struct ShadingUniform {
    f32 tint[4];
    f32 key_direction[4];
};

constexpr u32 kBrushColour = 0xFFB4A08Cu;       // ABGR: a warm grey.
constexpr u32 kSelectedColour = 0xFF3CC8FFu;    // Amber.
constexpr u32 kEntityColour = 0xFF60E080u;      // Green.

/// How many grid lines are worth drawing before the grid stops being a grid and
/// becomes a grey wash. Past this the spacing is doubled until it fits.
constexpr i32 kMaxGridLines = 260;

/// A brush's face edges, straight into the line buffer.
///
/// The public append_solid_edges builds Line structs for a caller that wants
/// them; this one skips that step, because the mesh rebuild does it for every
/// brush in the level and the intermediate vector is pure cost.
void append_edges(const map::Solid& solid, u32 colour, std::vector<LineVertex>& out);

void put(std::vector<LineVertex>& out, const Vec3& position, u32 colour) {
    LineVertex vertex{};
    vertex.position[0] = position.x;
    vertex.position[1] = position.y;
    vertex.position[2] = position.z;
    vertex.colour[0] = static_cast<u8>(colour & 0xFFu);
    vertex.colour[1] = static_cast<u8>((colour >> 8) & 0xFFu);
    vertex.colour[2] = static_cast<u8>((colour >> 16) & 0xFFu);
    vertex.colour[3] = static_cast<u8>((colour >> 24) & 0xFFu);
    out.push_back(vertex);
}

void append_edges(const map::Solid& solid, u32 colour, std::vector<LineVertex>& out) {
    for (const map::Face& face : map::faces_of(solid)) {
        const std::vector<Vec3d>& points = face.winding.points();
        for (usize i = 0; i < points.size(); ++i) {
            const Vec3d& from = points[i];
            const Vec3d& to = points[(i + 1) % points.size()];
            put(out, Vec3(static_cast<f32>(from.x), static_cast<f32>(from.y),
                          static_cast<f32>(from.z)), colour);
            put(out, Vec3(static_cast<f32>(to.x), static_cast<f32>(to.y),
                          static_cast<f32>(to.z)), colour);
        }
    }
}

}  // namespace

// ---------------------------------------------------------------------------
// Geometry, without a GPU in sight
// ---------------------------------------------------------------------------

void append_solid_edges(std::vector<ViewportRenderer::Line>& lines,
                        const map::Solid& solid, u32 colour) {
    for (const map::Face& face : map::faces_of(solid)) {
        const std::vector<Vec3d>& points = face.winding.points();
        for (usize i = 0; i < points.size(); ++i) {
            const Vec3d& from = points[i];
            const Vec3d& to = points[(i + 1) % points.size()];
            lines.push_back(ViewportRenderer::Line{
                Vec3(static_cast<f32>(from.x), static_cast<f32>(from.y),
                     static_cast<f32>(from.z)),
                Vec3(static_cast<f32>(to.x), static_cast<f32>(to.y),
                     static_cast<f32>(to.z)),
                colour});
        }
    }
}

std::vector<ViewportRenderer::Line> grid_lines(const Viewport& view, f32 spacing) {
    std::vector<ViewportRenderer::Line> lines;
    if (!is_orthographic(view.kind) || spacing <= 0.0f) {
        return lines;
    }

    const ViewAxes axes = view.axes();
    const f32 units_across = view.width / std::max(view.zoom, 0.0001f);
    const f32 units_down = view.height / std::max(view.zoom, 0.0001f);

    // A grid drawn at four pixels a line is a grey wash, not a grid. Doubling
    // the spacing until it is readable keeps it useful at every zoom -- which
    // is the whole reason to have one.
    f32 step = spacing;
    while ((units_across / step) > static_cast<f32>(kMaxGridLines) ||
           (units_down / step) > static_cast<f32>(kMaxGridLines)) {
        step *= 2.0f;
    }

    const f32 half_across = units_across * 0.5f;
    const f32 half_down = units_down * 0.5f;
    const f32 centre_across = dot(view.centre, axes.right);
    const f32 centre_down = dot(view.centre, axes.up);

    const f32 first_across = std::floor((centre_across - half_across) / step) * step;
    const f32 last_across = centre_across + half_across;
    const f32 first_down = std::floor((centre_down - half_down) / step) * step;
    const f32 last_down = centre_down + half_down;

    // Every eighth line brighter, and the axes brighter still, so you can count
    // squares without measuring them.
    const auto colour_for = [step](f32 value) -> u32 {
        if (std::abs(value) < step * 0.5f) {
            return 0xFF7A7AC8u;  // The axis itself.
        }
        return std::fmod(std::abs(value), step * 8.0f) < step * 0.5f ? 0xFF4A4A4Au
                                                                    : 0xFF303030u;
    };

    for (f32 across = first_across; across <= last_across; across += step) {
        const Vec3 base = axes.right * across;
        lines.push_back(ViewportRenderer::Line{base + axes.up * first_down,
                                               base + axes.up * last_down,
                                               colour_for(across)});
    }
    for (f32 down = first_down; down <= last_down; down += step) {
        const Vec3 base = axes.up * down;
        lines.push_back(ViewportRenderer::Line{base + axes.right * first_across,
                                               base + axes.right * last_across,
                                               colour_for(down)});
    }

    return lines;
}

// ---------------------------------------------------------------------------
// The GPU side
// ---------------------------------------------------------------------------

struct ViewportRenderer::Impl {
    SDL_GPUDevice* device = nullptr;
    SDL_GPUGraphicsPipeline* line_pipeline = nullptr;
    SDL_GPUGraphicsPipeline* solid_pipeline = nullptr;
    SDL_GPUSampler* sampler = nullptr;
    SDL_GPUTextureFormat depth_format = SDL_GPU_TEXTUREFORMAT_D32_FLOAT;

    /// One target per pane. Recreated when a pane is resized.
    struct Target {
        SDL_GPUTexture* colour = nullptr;
        SDL_GPUTexture* depth = nullptr;
        u32 width = 0;
        u32 height = 0;
    };
    std::vector<Target> targets;

    struct Buffer {
        SDL_GPUBuffer* handle = nullptr;
        u32 capacity = 0;
    };
    Buffer lines;
    Buffer solids;

    std::unordered_map<std::string, SDL_GPUTexture*> textures;

    // The cached mesh, rebuilt when the map or the selection changes rather
    // than every frame. An editor spends most of its time idle with the same
    // level on screen, so this is the difference between a warm laptop and a
    // cool one.
    u64 built_revision = 0;
    std::vector<i32> built_solids;
    std::vector<i32> built_entities;
    bool built = false;

    std::vector<LineVertex> line_vertices;
    std::vector<SolidVertex> solid_vertices;
    struct MaterialRange {
        std::string material;
        u32 first = 0;
        u32 count = 0;
    };
    std::vector<MaterialRange> solid_batches;

    /// Scratch, reused so the per-frame overlay costs no allocations after the
    /// first few frames.
    std::vector<LineVertex> frame_lines;

    ~Impl();

    [[nodiscard]] bool ensure_buffer(Buffer& buffer, u32 bytes, SDL_GPUBufferUsageFlags usage);
    void upload(SDL_GPUCommandBuffer* command, Buffer& buffer, const void* data, u32 bytes);
    [[nodiscard]] Target& ensure_target(usize index, u32 width, u32 height);
    [[nodiscard]] SDL_GPUTexture* texture_for(SDL_GPUCommandBuffer* command,
                                              const std::string& material);
    void rebuild(const Document& document);
};

ViewportRenderer::Impl::~Impl() {
    if (device == nullptr) {
        return;
    }
    SDL_WaitForGPUIdle(device);
    for (Target& target : targets) {
        if (target.colour != nullptr) {
            SDL_ReleaseGPUTexture(device, target.colour);
        }
        if (target.depth != nullptr) {
            SDL_ReleaseGPUTexture(device, target.depth);
        }
    }
    for (auto& [name, texture] : textures) {
        SDL_ReleaseGPUTexture(device, texture);
    }
    if (lines.handle != nullptr) {
        SDL_ReleaseGPUBuffer(device, lines.handle);
    }
    if (solids.handle != nullptr) {
        SDL_ReleaseGPUBuffer(device, solids.handle);
    }
    if (sampler != nullptr) {
        SDL_ReleaseGPUSampler(device, sampler);
    }
    if (line_pipeline != nullptr) {
        SDL_ReleaseGPUGraphicsPipeline(device, line_pipeline);
    }
    if (solid_pipeline != nullptr) {
        SDL_ReleaseGPUGraphicsPipeline(device, solid_pipeline);
    }
}

bool ViewportRenderer::Impl::ensure_buffer(Buffer& buffer, u32 bytes,
                                           SDL_GPUBufferUsageFlags usage) {
    if (bytes == 0) {
        return true;
    }
    if (buffer.handle != nullptr && buffer.capacity >= bytes) {
        return true;
    }

    if (buffer.handle != nullptr) {
        SDL_WaitForGPUIdle(device);
        SDL_ReleaseGPUBuffer(device, buffer.handle);
    }
    // Grown with headroom, so a level being built up a brush at a time does not
    // reallocate on every edit.
    buffer.capacity = std::max(bytes * 2u, 64u * 1024u);

    SDL_GPUBufferCreateInfo info{};
    info.usage = usage;
    info.size = buffer.capacity;
    buffer.handle = SDL_CreateGPUBuffer(device, &info);
    return buffer.handle != nullptr;
}

void ViewportRenderer::Impl::upload(SDL_GPUCommandBuffer* command, Buffer& buffer,
                                    const void* data, u32 bytes) {
    if (bytes == 0 || buffer.handle == nullptr) {
        return;
    }

    SDL_GPUTransferBufferCreateInfo info{};
    info.usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD;
    info.size = bytes;
    SDL_GPUTransferBuffer* transfer = SDL_CreateGPUTransferBuffer(device, &info);
    if (transfer == nullptr) {
        return;
    }

    void* mapped = SDL_MapGPUTransferBuffer(device, transfer, false);
    std::memcpy(mapped, data, bytes);
    SDL_UnmapGPUTransferBuffer(device, transfer);

    SDL_GPUCopyPass* copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUTransferBufferLocation source{transfer, 0};
    SDL_GPUBufferRegion destination{buffer.handle, 0, bytes};
    SDL_UploadToGPUBuffer(copy, &source, &destination, false);
    SDL_EndGPUCopyPass(copy);

    SDL_ReleaseGPUTransferBuffer(device, transfer);
}

ViewportRenderer::Impl::Target& ViewportRenderer::Impl::ensure_target(usize index,
                                                                     u32 width,
                                                                     u32 height) {
    if (index >= targets.size()) {
        targets.resize(index + 1);
    }
    Target& target = targets[index];
    if (target.colour != nullptr && target.width == width && target.height == height) {
        return target;
    }

    SDL_WaitForGPUIdle(device);
    if (target.colour != nullptr) {
        SDL_ReleaseGPUTexture(device, target.colour);
    }
    if (target.depth != nullptr) {
        SDL_ReleaseGPUTexture(device, target.depth);
    }

    SDL_GPUTextureCreateInfo colour{};
    colour.type = SDL_GPU_TEXTURETYPE_2D;
    colour.format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM;
    colour.usage = SDL_GPU_TEXTUREUSAGE_COLOR_TARGET | SDL_GPU_TEXTUREUSAGE_SAMPLER;
    colour.width = width;
    colour.height = height;
    colour.layer_count_or_depth = 1;
    colour.num_levels = 1;
    target.colour = SDL_CreateGPUTexture(device, &colour);

    SDL_GPUTextureCreateInfo depth{};
    depth.type = SDL_GPU_TEXTURETYPE_2D;
    depth.format = depth_format;
    depth.usage = SDL_GPU_TEXTUREUSAGE_DEPTH_STENCIL_TARGET;
    depth.width = width;
    depth.height = height;
    depth.layer_count_or_depth = 1;
    depth.num_levels = 1;
    target.depth = SDL_CreateGPUTexture(device, &depth);

    target.width = width;
    target.height = height;
    return target;
}

SDL_GPUTexture* ViewportRenderer::Impl::texture_for(SDL_GPUCommandBuffer* command,
                                                    const std::string& material) {
    if (const auto found = textures.find(material); found != textures.end()) {
        return found->second;
    }

    SDL_GPUTextureCreateInfo info{};
    info.type = SDL_GPU_TEXTURETYPE_2D;
    info.format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM;
    info.usage = SDL_GPU_TEXTUREUSAGE_SAMPLER;
    info.width = asset::kDevTextureSize;
    info.height = asset::kDevTextureSize;
    info.layer_count_or_depth = 1;
    info.num_levels = 1;

    SDL_GPUTexture* texture = SDL_CreateGPUTexture(device, &info);
    if (texture == nullptr) {
        return nullptr;
    }

    // The same generator the engine uses, so a surface reads the same in the
    // editor as it will in the game.
    const std::vector<u8> pixels = asset::dev_texture(material);

    SDL_GPUTransferBufferCreateInfo transfer_info{};
    transfer_info.usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD;
    transfer_info.size = static_cast<u32>(pixels.size());
    SDL_GPUTransferBuffer* transfer = SDL_CreateGPUTransferBuffer(device, &transfer_info);
    void* mapped = SDL_MapGPUTransferBuffer(device, transfer, false);
    std::memcpy(mapped, pixels.data(), pixels.size());
    SDL_UnmapGPUTransferBuffer(device, transfer);

    SDL_GPUCopyPass* copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUTextureTransferInfo source{};
    source.transfer_buffer = transfer;
    SDL_GPUTextureRegion region{};
    region.texture = texture;
    region.w = asset::kDevTextureSize;
    region.h = asset::kDevTextureSize;
    region.d = 1;
    SDL_UploadToGPUTexture(copy, &source, &region, false);
    SDL_EndGPUCopyPass(copy);
    SDL_ReleaseGPUTransferBuffer(device, transfer);

    textures.emplace(material, texture);
    return texture;
}

void ViewportRenderer::Impl::rebuild(const Document& document) {
    line_vertices.clear();
    solid_vertices.clear();
    solid_batches.clear();

    const Selection& selection = document.selection();

    // Solid faces, gathered per material so the texture is bound once per run.
    std::unordered_map<std::string, std::vector<SolidVertex>> by_material;

    for (const Document::SolidRef& ref : document.all_solids()) {
        const map::Solid& solid = *ref.solid;
        const bool selected = selection.contains_solid(solid.id);

        append_edges(solid, selected ? kSelectedColour : kBrushColour,
                     line_vertices);

        for (const map::Face& face : map::faces_of(solid)) {
            const map::Side& side = solid.sides[face.side];
            std::vector<SolidVertex>& target = by_material[side.material];

            const Vec3d normal = side.plane.normal;
            const std::vector<Vec3d>& points = face.winding.points();

            // A fan from the first point. Valid for any convex polygon, and a
            // winding is only ever convex.
            for (usize i = 2; i < points.size(); ++i) {
                for (const Vec3d& point : {points[0], points[i - 1], points[i]}) {
                    SolidVertex vertex{};
                    vertex.position[0] = static_cast<f32>(point.x);
                    vertex.position[1] = static_cast<f32>(point.y);
                    vertex.position[2] = static_cast<f32>(point.z);
                    // The texture axes carry their own scale, as they do in the
                    // compiled format.
                    const f64 u = dot(point, side.uaxis.axis) / side.uaxis.scale +
                                  side.uaxis.shift;
                    const f64 v = dot(point, side.vaxis.axis) / side.vaxis.scale +
                                  side.vaxis.shift;
                    vertex.uv[0] = static_cast<f32>(u) /
                                   static_cast<f32>(asset::kDevTextureSize);
                    vertex.uv[1] = static_cast<f32>(v) /
                                   static_cast<f32>(asset::kDevTextureSize);
                    vertex.normal[0] = static_cast<f32>(normal.x);
                    vertex.normal[1] = static_cast<f32>(normal.y);
                    vertex.normal[2] = static_cast<f32>(normal.z);
                    target.push_back(vertex);
                }
            }
        }
    }

    for (auto& [material, vertices] : by_material) {
        MaterialRange range;
        range.material = material;
        range.first = static_cast<u32>(solid_vertices.size());
        range.count = static_cast<u32>(vertices.size());
        solid_vertices.insert(solid_vertices.end(), vertices.begin(), vertices.end());
        solid_batches.push_back(std::move(range));
    }

    // Point entities as a small box, so there is something to see and click.
    for (const map::Entity& entity : document.map().entities) {
        if (entity.is_brush_entity()) {
            continue;
        }
        const std::optional<Vec3d> origin = entity.origin();
        if (!origin) {
            continue;
        }
        const bool selected = selection.contains_entity(entity.id);
        const u32 colour = selected ? kSelectedColour : kEntityColour;
        const Vec3 centre(static_cast<f32>(origin->x), static_cast<f32>(origin->y),
                          static_cast<f32>(origin->z));
        constexpr f32 kHalf = 8.0f;

        for (usize axis = 0; axis < 3; ++axis) {
            for (i32 a = -1; a <= 1; a += 2) {
                for (i32 b = -1; b <= 1; b += 2) {
                    Vec3 from = centre;
                    Vec3 to = centre;
                    const usize u = (axis + 1) % 3;
                    const usize v = (axis + 2) % 3;
                    from[u] = to[u] = centre[u] + static_cast<f32>(a) * kHalf;
                    from[v] = to[v] = centre[v] + static_cast<f32>(b) * kHalf;
                    from[axis] = centre[axis] - kHalf;
                    to[axis] = centre[axis] + kHalf;
                    put(line_vertices, from, colour);
                    put(line_vertices, to, colour);
                }
            }
        }
    }

    built_revision = document.revision();
    built_solids = selection.solids;
    built_entities = selection.entities;
    built = true;
}

// ---------------------------------------------------------------------------
// Setup and drawing
// ---------------------------------------------------------------------------

namespace {

SDL_GPUShader* load_shader(SDL_GPUDevice* device, const u8* code, usize size,
                           SDL_GPUShaderStage stage, u32 samplers, u32 uniforms) {
    SDL_GPUShaderCreateInfo info{};
    info.code = code;
    info.code_size = size;
    info.entrypoint = "main";
    info.format = SDL_GPU_SHADERFORMAT_SPIRV;
    info.stage = stage;
    info.num_samplers = samplers;
    info.num_uniform_buffers = uniforms;
    return SDL_CreateGPUShader(device, &info);
}

}  // namespace

ViewportRenderer::ViewportRenderer(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
ViewportRenderer::~ViewportRenderer() = default;

std::expected<std::unique_ptr<ViewportRenderer>, std::string> ViewportRenderer::create(
    SDL_GPUDevice* device) {
    auto impl = std::make_unique<Impl>();
    impl->device = device;

    for (const SDL_GPUTextureFormat candidate :
         {SDL_GPU_TEXTUREFORMAT_D32_FLOAT, SDL_GPU_TEXTUREFORMAT_D24_UNORM,
          SDL_GPU_TEXTUREFORMAT_D16_UNORM}) {
        if (SDL_GPUTextureSupportsFormat(device, candidate, SDL_GPU_TEXTURETYPE_2D,
                                         SDL_GPU_TEXTUREUSAGE_DEPTH_STENCIL_TARGET)) {
            impl->depth_format = candidate;
            break;
        }
    }

    SDL_GPUColorTargetDescription colour{};
    colour.format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM;

    // --- Lines -------------------------------------------------------------

    SDL_GPUShader* line_vertex =
        load_shader(device, shaders::chisel_line_vert.data(),
                    shaders::chisel_line_vert.size(), SDL_GPU_SHADERSTAGE_VERTEX, 0, 1);
    SDL_GPUShader* line_fragment =
        load_shader(device, shaders::chisel_line_frag.data(),
                    shaders::chisel_line_frag.size(), SDL_GPU_SHADERSTAGE_FRAGMENT, 0, 0);
    if (line_vertex == nullptr || line_fragment == nullptr) {
        return std::unexpected(
            std::format("could not create the line shaders: {}", SDL_GetError()));
    }

    SDL_GPUVertexBufferDescription line_buffer{};
    line_buffer.slot = 0;
    line_buffer.pitch = sizeof(LineVertex);
    line_buffer.input_rate = SDL_GPU_VERTEXINPUTRATE_VERTEX;

    const SDL_GPUVertexAttribute line_attributes[2] = {
        {0, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, offsetof(LineVertex, position)},
        {1, 0, SDL_GPU_VERTEXELEMENTFORMAT_UBYTE4_NORM, offsetof(LineVertex, colour)},
    };

    SDL_GPUGraphicsPipelineCreateInfo line_info{};
    line_info.vertex_shader = line_vertex;
    line_info.fragment_shader = line_fragment;
    line_info.vertex_input_state.vertex_buffer_descriptions = &line_buffer;
    line_info.vertex_input_state.num_vertex_buffers = 1;
    line_info.vertex_input_state.vertex_attributes = line_attributes;
    line_info.vertex_input_state.num_vertex_attributes = 2;
    line_info.primitive_type = SDL_GPU_PRIMITIVETYPE_LINELIST;
    line_info.rasterizer_state.fill_mode = SDL_GPU_FILLMODE_FILL;
    line_info.rasterizer_state.cull_mode = SDL_GPU_CULLMODE_NONE;
    // Lines are drawn over the solid pass and tested but not written: a
    // wireframe that fought the depth buffer with itself would shimmer.
    line_info.depth_stencil_state.enable_depth_test = true;
    line_info.depth_stencil_state.enable_depth_write = false;
    line_info.depth_stencil_state.compare_op = SDL_GPU_COMPAREOP_LESS_OR_EQUAL;
    line_info.target_info.color_target_descriptions = &colour;
    line_info.target_info.num_color_targets = 1;
    line_info.target_info.depth_stencil_format = impl->depth_format;
    line_info.target_info.has_depth_stencil_target = true;

    impl->line_pipeline = SDL_CreateGPUGraphicsPipeline(device, &line_info);
    SDL_ReleaseGPUShader(device, line_vertex);
    SDL_ReleaseGPUShader(device, line_fragment);
    if (impl->line_pipeline == nullptr) {
        return std::unexpected(
            std::format("could not create the line pipeline: {}", SDL_GetError()));
    }

    // --- Solid -------------------------------------------------------------

    SDL_GPUShader* solid_vertex =
        load_shader(device, shaders::chisel_solid_vert.data(),
                    shaders::chisel_solid_vert.size(), SDL_GPU_SHADERSTAGE_VERTEX, 0, 1);
    SDL_GPUShader* solid_fragment =
        load_shader(device, shaders::chisel_solid_frag.data(),
                    shaders::chisel_solid_frag.size(), SDL_GPU_SHADERSTAGE_FRAGMENT, 1, 1);
    if (solid_vertex == nullptr || solid_fragment == nullptr) {
        return std::unexpected(
            std::format("could not create the solid shaders: {}", SDL_GetError()));
    }

    SDL_GPUVertexBufferDescription solid_buffer{};
    solid_buffer.slot = 0;
    solid_buffer.pitch = sizeof(SolidVertex);
    solid_buffer.input_rate = SDL_GPU_VERTEXINPUTRATE_VERTEX;

    const SDL_GPUVertexAttribute solid_attributes[3] = {
        {0, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, offsetof(SolidVertex, position)},
        {1, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT2, offsetof(SolidVertex, uv)},
        {2, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, offsetof(SolidVertex, normal)},
    };

    SDL_GPUGraphicsPipelineCreateInfo solid_info{};
    solid_info.vertex_shader = solid_vertex;
    solid_info.fragment_shader = solid_fragment;
    solid_info.vertex_input_state.vertex_buffer_descriptions = &solid_buffer;
    solid_info.vertex_input_state.num_vertex_buffers = 1;
    solid_info.vertex_input_state.vertex_attributes = solid_attributes;
    solid_info.vertex_input_state.num_vertex_attributes = 3;
    solid_info.primitive_type = SDL_GPU_PRIMITIVETYPE_TRIANGLELIST;
    solid_info.rasterizer_state.fill_mode = SDL_GPU_FILLMODE_FILL;
    // No culling. A designer inside a room is looking at the back of its walls,
    // and an editor that hid them would be hiding the level.
    solid_info.rasterizer_state.cull_mode = SDL_GPU_CULLMODE_NONE;
    solid_info.depth_stencil_state.enable_depth_test = true;
    solid_info.depth_stencil_state.enable_depth_write = true;
    solid_info.depth_stencil_state.compare_op = SDL_GPU_COMPAREOP_LESS;
    solid_info.target_info.color_target_descriptions = &colour;
    solid_info.target_info.num_color_targets = 1;
    solid_info.target_info.depth_stencil_format = impl->depth_format;
    solid_info.target_info.has_depth_stencil_target = true;

    impl->solid_pipeline = SDL_CreateGPUGraphicsPipeline(device, &solid_info);
    SDL_ReleaseGPUShader(device, solid_vertex);
    SDL_ReleaseGPUShader(device, solid_fragment);
    if (impl->solid_pipeline == nullptr) {
        return std::unexpected(
            std::format("could not create the solid pipeline: {}", SDL_GetError()));
    }

    SDL_GPUSamplerCreateInfo sampler{};
    sampler.min_filter = SDL_GPU_FILTER_LINEAR;
    sampler.mag_filter = SDL_GPU_FILTER_NEAREST;
    sampler.mipmap_mode = SDL_GPU_SAMPLERMIPMAPMODE_LINEAR;
    sampler.address_mode_u = SDL_GPU_SAMPLERADDRESSMODE_REPEAT;
    sampler.address_mode_v = SDL_GPU_SAMPLERADDRESSMODE_REPEAT;
    sampler.address_mode_w = SDL_GPU_SAMPLERADDRESSMODE_REPEAT;
    impl->sampler = SDL_CreateGPUSampler(device, &sampler);
    if (impl->sampler == nullptr) {
        return std::unexpected(
            std::format("could not create a sampler: {}", SDL_GetError()));
    }

    return std::unique_ptr<ViewportRenderer>(new ViewportRenderer(std::move(impl)));
}

SDL_GPUTexture* ViewportRenderer::material_texture(SDL_GPUCommandBuffer* command,
                                                   const std::string& material) {
    return impl_->texture_for(command, material);
}

SDL_GPUTexture* ViewportRenderer::draw(SDL_GPUCommandBuffer* command, usize index,
                                       const Viewport& view, const Document& document,
                                       const std::vector<Line>& overlay) {
    Impl& impl = *impl_;
    stats_ = Stats{};

    const auto width = static_cast<u32>(std::max(view.width, 1.0f));
    const auto height = static_cast<u32>(std::max(view.height, 1.0f));
    if (width < 8 || height < 8) {
        return nullptr;  // A pane dragged shut.
    }

    Impl::Target& target = impl.ensure_target(index, width, height);
    if (target.colour == nullptr || target.depth == nullptr) {
        return nullptr;
    }

    // Rebuilt only when the map or the selection has actually moved.
    if (!impl.built || impl.built_revision != document.revision() ||
        impl.built_solids != document.selection().solids ||
        impl.built_entities != document.selection().entities) {
        impl.rebuild(document);
        const auto solid_bytes =
            static_cast<u32>(impl.solid_vertices.size() * sizeof(SolidVertex));
        if (impl.ensure_buffer(impl.solids, solid_bytes, SDL_GPU_BUFFERUSAGE_VERTEX)) {
            impl.upload(command, impl.solids, impl.solid_vertices.data(), solid_bytes);
        } else {
            // Out of device memory. Drop the mesh so nothing is drawn from a
            // buffer that does not exist, and leave the cache invalid so the
            // next frame tries again rather than showing an empty level for
            // good.
            impl.built = false;
            impl.solid_vertices.clear();
            impl.solid_batches.clear();
        }
    }

    // The overlay and the wireframe share one buffer, refilled each frame: the
    // grid moves whenever the view does, so there is nothing to cache.
    impl.frame_lines = impl.line_vertices;
    for (const Line& line : overlay) {
        put(impl.frame_lines, line.from, line.colour);
        put(impl.frame_lines, line.to, line.colour);
    }
    const auto line_bytes = static_cast<u32>(impl.frame_lines.size() * sizeof(LineVertex));
    if (impl.ensure_buffer(impl.lines, line_bytes, SDL_GPU_BUFFERUSAGE_VERTEX)) {
        impl.upload(command, impl.lines, impl.frame_lines.data(), line_bytes);
    } else {
        impl.frame_lines.clear();  // Same again: no buffer, no wireframe.
    }

    // Materials are created lazily, and the copy passes above have already
    // ended -- SDL_GPU allows only one pass at a time on a command buffer.
    std::vector<SDL_GPUTexture*> batch_textures;
    batch_textures.reserve(impl.solid_batches.size());
    for (const Impl::MaterialRange& batch : impl.solid_batches) {
        batch_textures.push_back(impl.texture_for(command, batch.material));
    }

    const Mat4 view_projection = view.view_projection();
    CameraUniform camera{};
    std::memcpy(camera.view_projection, view_projection.data(),
                sizeof(camera.view_projection));
    camera.tint[0] = camera.tint[1] = camera.tint[2] = camera.tint[3] = 1.0f;

    SDL_GPUColorTargetInfo colour{};
    colour.texture = target.colour;
    colour.clear_color = is_orthographic(view.kind)
                             ? SDL_FColor{0.11f, 0.11f, 0.12f, 1.0f}
                             : SDL_FColor{0.05f, 0.06f, 0.08f, 1.0f};
    colour.load_op = SDL_GPU_LOADOP_CLEAR;
    colour.store_op = SDL_GPU_STOREOP_STORE;

    SDL_GPUDepthStencilTargetInfo depth{};
    depth.texture = target.depth;
    depth.clear_depth = 1.0f;
    depth.load_op = SDL_GPU_LOADOP_CLEAR;
    depth.store_op = SDL_GPU_STOREOP_DONT_CARE;

    SDL_GPURenderPass* pass = SDL_BeginGPURenderPass(command, &colour, 1, &depth);

    // Solid faces, in the 3D view only. The orthographic views are wireframe on
    // purpose: a filled top view of a sealed level is a grey rectangle, and you
    // build on a grid by seeing through what is in front of you.
    if (view.kind == ViewKind::Perspective && !impl.solid_vertices.empty() &&
        impl.solids.handle != nullptr) {
        SDL_BindGPUGraphicsPipeline(pass, impl.solid_pipeline);
        SDL_GPUBufferBinding binding{impl.solids.handle, 0};
        SDL_BindGPUVertexBuffers(pass, 0, &binding, 1);
        SDL_PushGPUVertexUniformData(command, 0, &camera, sizeof(camera));

        ShadingUniform shading{};
        shading.tint[0] = shading.tint[1] = shading.tint[2] = shading.tint[3] = 1.0f;
        shading.key_direction[0] = 0.35f;
        shading.key_direction[1] = 0.25f;
        shading.key_direction[2] = 0.90f;
        SDL_PushGPUFragmentUniformData(command, 0, &shading, sizeof(shading));

        for (usize i = 0; i < impl.solid_batches.size(); ++i) {
            if (batch_textures[i] == nullptr) {
                continue;
            }
            SDL_GPUTextureSamplerBinding texture{};
            texture.texture = batch_textures[i];
            texture.sampler = impl.sampler;
            SDL_BindGPUFragmentSamplers(pass, 0, &texture, 1);
            SDL_DrawGPUPrimitives(pass, impl.solid_batches[i].count, 1,
                                  impl.solid_batches[i].first, 0);
            stats_.triangles += impl.solid_batches[i].count / 3;
        }
    }

    if (!impl.frame_lines.empty() && impl.lines.handle != nullptr) {
        SDL_BindGPUGraphicsPipeline(pass, impl.line_pipeline);
        SDL_GPUBufferBinding binding{impl.lines.handle, 0};
        SDL_BindGPUVertexBuffers(pass, 0, &binding, 1);
        SDL_PushGPUVertexUniformData(command, 0, &camera, sizeof(camera));
        SDL_DrawGPUPrimitives(pass, static_cast<u32>(impl.frame_lines.size()), 1, 0, 0);
        stats_.lines = impl.frame_lines.size() / 2;
    }

    SDL_EndGPURenderPass(pass);
    return target.colour;
}

}  // namespace kero::chisel
