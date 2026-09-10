// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "render/renderer.hpp"

#include "asset/devtexture.hpp"
#include "core/log.hpp"
#include "math/units.hpp"

#include <SDL3/SDL.h>

#include <algorithm>
#include <cstring>
#include <format>
#include <unordered_map>

#include "world_vert.hpp"
#include "world_frag.hpp"

namespace kero::render {
namespace {

KERO_LOG_CATEGORY(log, "render");

/// One vertex of world geometry. Position, texture coordinate, normal -- and
/// nothing else until Radiance produces lightmaps to add a second UV for.
struct Vertex {
    f32 position[3];
    f32 uv[2];
    f32 normal[3];
};
static_assert(sizeof(Vertex) == 32);

struct CameraUniform {
    f32 view_projection[16];
};

struct ShadingUniform {
    f32 tint[4];
    f32 key_direction[4];
};

/// Where one face's triangles live in the index buffer.
struct FaceRange {
    u32 first_index = 0;
    u32 index_count = 0;
    u32 material = 0;
    u32 source_face = 0;
};

/// A run of faces sharing a material, so the texture is bound once per run
/// rather than once per face.
struct MaterialBatch {
    u32 material = 0;
    u32 first_face = 0;   ///< Into the sorted face list.
    u32 face_count = 0;
};

}  // namespace

struct Renderer::Impl {
    SDL_Window* window = nullptr;
    SDL_GPUDevice* device = nullptr;
    SDL_GPUGraphicsPipeline* pipeline = nullptr;
    SDL_GPUSampler* sampler = nullptr;

    SDL_GPUBuffer* vertex_buffer = nullptr;
    SDL_GPUBuffer* index_buffer = nullptr;
    SDL_GPUTexture* depth_texture = nullptr;
    u32 depth_width = 0;
    u32 depth_height = 0;
    SDL_GPUTextureFormat depth_format = SDL_GPU_TEXTUREFORMAT_D32_FLOAT;

    std::vector<SDL_GPUTexture*> textures;
    std::vector<FaceRange> faces;         ///< Sorted by material.
    std::vector<MaterialBatch> batches;
    std::vector<u32> face_of_source;      ///< Original face index -> sorted position.
    std::vector<u8> visible;              ///< Per sorted face, this frame.
    std::vector<u8> pvs;

    u32 index_count = 0;

    ~Impl() {
        if (device == nullptr) {
            return;
        }
        for (SDL_GPUTexture* texture : textures) {
            SDL_ReleaseGPUTexture(device, texture);
        }
        if (depth_texture != nullptr) {
            SDL_ReleaseGPUTexture(device, depth_texture);
        }
        if (vertex_buffer != nullptr) {
            SDL_ReleaseGPUBuffer(device, vertex_buffer);
        }
        if (index_buffer != nullptr) {
            SDL_ReleaseGPUBuffer(device, index_buffer);
        }
        if (sampler != nullptr) {
            SDL_ReleaseGPUSampler(device, sampler);
        }
        if (pipeline != nullptr) {
            SDL_ReleaseGPUGraphicsPipeline(device, pipeline);
        }
        if (window != nullptr) {
            SDL_ReleaseWindowFromGPUDevice(device, window);
        }
        SDL_DestroyGPUDevice(device);
        if (window != nullptr) {
            SDL_DestroyWindow(window);
        }
    }
};

Renderer::Renderer(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}

Renderer::~Renderer() = default;

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

std::expected<std::unique_ptr<Renderer>, std::string> Renderer::create(
    std::string_view title, i32 width, i32 height) {
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        return std::unexpected(std::format("SDL_Init failed: {}", SDL_GetError()));
    }

    auto impl = std::make_unique<Impl>();

    impl->window = SDL_CreateWindow(std::string(title).c_str(), width, height,
                                    SDL_WINDOW_RESIZABLE);
    if (impl->window == nullptr) {
        return std::unexpected(std::format("could not open a window: {}", SDL_GetError()));
    }

    // SPIR-V only: the shaders are compiled at build time and this is the one
    // format they are in. On Windows and macOS SDL translates it.
    impl->device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, nullptr);
    if (impl->device == nullptr) {
        return std::unexpected(std::format(
            "no usable GPU device: {}. A Vulkan driver is needed on Linux",
            SDL_GetError()));
    }
    if (!SDL_ClaimWindowForGPUDevice(impl->device, impl->window)) {
        return std::unexpected(
            std::format("could not attach the window to the GPU device: {}",
                        SDL_GetError()));
    }

    KERO_INFO(log, "GPU backend: {}", SDL_GetGPUDeviceDriver(impl->device));

    SDL_GPUShader* vertex =
        load_shader(impl->device, kero::shaders::world_vert.data(),
                    kero::shaders::world_vert.size(), SDL_GPU_SHADERSTAGE_VERTEX, 0, 1);
    SDL_GPUShader* fragment =
        load_shader(impl->device, kero::shaders::world_frag.data(),
                    kero::shaders::world_frag.size(), SDL_GPU_SHADERSTAGE_FRAGMENT, 1, 1);
    if (vertex == nullptr || fragment == nullptr) {
        return std::unexpected(std::format("could not create the world shaders: {}",
                                           SDL_GetError()));
    }

    // 32-bit depth if the device has it: a level is 16384 ku across and a
    // 24-bit depth buffer shows its seams at that range.
    for (SDL_GPUTextureFormat candidate :
         {SDL_GPU_TEXTUREFORMAT_D32_FLOAT, SDL_GPU_TEXTUREFORMAT_D24_UNORM,
          SDL_GPU_TEXTUREFORMAT_D16_UNORM}) {
        if (SDL_GPUTextureSupportsFormat(impl->device, candidate,
                                         SDL_GPU_TEXTURETYPE_2D,
                                         SDL_GPU_TEXTUREUSAGE_DEPTH_STENCIL_TARGET)) {
            impl->depth_format = candidate;
            break;
        }
    }

    SDL_GPUVertexBufferDescription vertex_buffer_description{};
    vertex_buffer_description.slot = 0;
    vertex_buffer_description.pitch = sizeof(Vertex);
    vertex_buffer_description.input_rate = SDL_GPU_VERTEXINPUTRATE_VERTEX;

    const SDL_GPUVertexAttribute attributes[3] = {
        {0, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, offsetof(Vertex, position)},
        {1, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT2, offsetof(Vertex, uv)},
        {2, 0, SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, offsetof(Vertex, normal)},
    };

    SDL_GPUColorTargetDescription colour_target{};
    colour_target.format = SDL_GetGPUSwapchainTextureFormat(impl->device, impl->window);

    SDL_GPUGraphicsPipelineCreateInfo pipeline_info{};
    pipeline_info.vertex_shader = vertex;
    pipeline_info.fragment_shader = fragment;
    pipeline_info.vertex_input_state.vertex_buffer_descriptions =
        &vertex_buffer_description;
    pipeline_info.vertex_input_state.num_vertex_buffers = 1;
    pipeline_info.vertex_input_state.vertex_attributes = attributes;
    pipeline_info.vertex_input_state.num_vertex_attributes = 3;
    pipeline_info.primitive_type = SDL_GPU_PRIMITIVETYPE_TRIANGLELIST;
    pipeline_info.rasterizer_state.fill_mode = SDL_GPU_FILLMODE_FILL;
    // Back faces are inside solid geometry, so the depth buffer already hides
    // them; culling would only save fill rate, and getting the winding wrong
    // turns the level inside out. Enable it once there is a frame time worth
    // defending.
    pipeline_info.rasterizer_state.cull_mode = SDL_GPU_CULLMODE_NONE;
    pipeline_info.depth_stencil_state.enable_depth_test = true;
    pipeline_info.depth_stencil_state.enable_depth_write = true;
    pipeline_info.depth_stencil_state.compare_op = SDL_GPU_COMPAREOP_LESS;
    pipeline_info.target_info.color_target_descriptions = &colour_target;
    pipeline_info.target_info.num_color_targets = 1;
    pipeline_info.target_info.depth_stencil_format = impl->depth_format;
    pipeline_info.target_info.has_depth_stencil_target = true;

    impl->pipeline = SDL_CreateGPUGraphicsPipeline(impl->device, &pipeline_info);
    SDL_ReleaseGPUShader(impl->device, vertex);
    SDL_ReleaseGPUShader(impl->device, fragment);
    if (impl->pipeline == nullptr) {
        return std::unexpected(
            std::format("could not create the world pipeline: {}", SDL_GetError()));
    }

    SDL_GPUSamplerCreateInfo sampler_info{};
    // Nearest on magnification keeps the developer grid crisp when a wall is
    // close, which is the point of a measuring texture.
    sampler_info.min_filter = SDL_GPU_FILTER_LINEAR;
    sampler_info.mag_filter = SDL_GPU_FILTER_NEAREST;
    sampler_info.mipmap_mode = SDL_GPU_SAMPLERMIPMAPMODE_LINEAR;
    sampler_info.address_mode_u = SDL_GPU_SAMPLERADDRESSMODE_REPEAT;
    sampler_info.address_mode_v = SDL_GPU_SAMPLERADDRESSMODE_REPEAT;
    sampler_info.address_mode_w = SDL_GPU_SAMPLERADDRESSMODE_REPEAT;
    impl->sampler = SDL_CreateGPUSampler(impl->device, &sampler_info);
    if (impl->sampler == nullptr) {
        return std::unexpected(
            std::format("could not create a sampler: {}", SDL_GetError()));
    }

    auto renderer = std::unique_ptr<Renderer>(new Renderer(std::move(impl)));
    // The pointer is not captured until the window is clicked. Grabbing it the
    // moment a window opens is hostile to anyone who launched the thing to look
    // at it, and it makes an automated smoke test disruptive to whoever is
    // using the machine.
    renderer->set_mouse_captured(false);
    return renderer;
}

std::expected<void, std::string> Renderer::set_level(const bsp::Level& level) {
    Impl& impl = *impl_;

    // --- Build the geometry --------------------------------------------------

    std::vector<Vertex> vertices;
    std::vector<u32> indices;
    std::vector<FaceRange> faces;

    impl.face_of_source.assign(level.faces().size(), Index::kNone);

    // Sorted by material so the texture is bound once per run of faces rather
    // than once per face. On a real level this is the difference between a few
    // dozen draw calls and a few thousand.
    std::vector<u32> order;
    order.reserve(level.faces().size());
    for (u32 i = 0; i < level.faces().size(); ++i) {
        const bsp::DiskFace& face = level.faces()[i];
        if (any(static_cast<bsp::SurfaceFlags>(face.surface_flags) &
                bsp::SurfaceFlags::NoDraw)) {
            continue;
        }
        order.push_back(i);
    }
    std::ranges::sort(order, [&level](u32 a, u32 b) {
        return level.faces()[a].texinfo < level.faces()[b].texinfo;
    });

    for (u32 source : order) {
        const bsp::DiskFace& face = level.faces()[source];
        const bsp::DiskTexInfo& texinfo = level.texinfos()[face.texinfo];

        const bsp::DiskPlane& plane = level.planes()[face.plane];
        Vec3 normal(plane.normal[0], plane.normal[1], plane.normal[2]);
        if (face.flipped != 0) {
            normal = -normal;
        }

        const auto first_vertex = static_cast<u32>(vertices.size());
        for (u32 v = 0; v < face.vertex_count; ++v) {
            const bsp::DiskVertex& source_vertex =
                level.vertices()[level.face_vertices()[face.first_vertex + v]];
            const Vec3 position(source_vertex.position[0], source_vertex.position[1],
                                source_vertex.position[2]);

            Vertex vertex{};
            vertex.position[0] = position.x;
            vertex.position[1] = position.y;
            vertex.position[2] = position.z;
            // The texture axes already have the scale folded in, so this is one
            // dot product per coordinate and no divide.
            vertex.uv[0] = (position.x * texinfo.u_axis[0] + position.y * texinfo.u_axis[1] +
                            position.z * texinfo.u_axis[2] + texinfo.u_axis[3]) /
                           static_cast<f32>(asset::kDevTextureSize);
            vertex.uv[1] = (position.x * texinfo.v_axis[0] + position.y * texinfo.v_axis[1] +
                            position.z * texinfo.v_axis[2] + texinfo.v_axis[3]) /
                           static_cast<f32>(asset::kDevTextureSize);
            vertex.normal[0] = normal.x;
            vertex.normal[1] = normal.y;
            vertex.normal[2] = normal.z;
            vertices.push_back(vertex);
        }

        FaceRange range;
        range.first_index = static_cast<u32>(indices.size());
        range.material = face.texinfo;
        range.source_face = source;

        // A fan from the first vertex. Valid for any convex polygon, and a
        // winding is only ever convex.
        for (u32 v = 2; v < face.vertex_count; ++v) {
            indices.push_back(first_vertex);
            indices.push_back(first_vertex + v - 1);
            indices.push_back(first_vertex + v);
        }
        range.index_count = static_cast<u32>(indices.size()) - range.first_index;

        impl.face_of_source[source] = static_cast<u32>(faces.size());
        faces.push_back(range);
    }

    if (vertices.empty() || indices.empty()) {
        return std::unexpected("the level has no drawable faces");
    }

    impl.faces = std::move(faces);
    impl.index_count = static_cast<u32>(indices.size());
    impl.visible.assign(impl.faces.size(), 0);

    impl.batches.clear();
    for (u32 i = 0; i < impl.faces.size(); ++i) {
        if (impl.batches.empty() || impl.batches.back().material != impl.faces[i].material) {
            impl.batches.push_back(MaterialBatch{impl.faces[i].material, i, 0});
        }
        ++impl.batches.back().face_count;
    }

    // --- Upload --------------------------------------------------------------

    const auto vertex_bytes = static_cast<u32>(vertices.size() * sizeof(Vertex));
    const auto index_bytes = static_cast<u32>(indices.size() * sizeof(u32));

    SDL_GPUBufferCreateInfo vertex_info{};
    vertex_info.usage = SDL_GPU_BUFFERUSAGE_VERTEX;
    vertex_info.size = vertex_bytes;
    impl.vertex_buffer = SDL_CreateGPUBuffer(impl.device, &vertex_info);

    SDL_GPUBufferCreateInfo index_info{};
    index_info.usage = SDL_GPU_BUFFERUSAGE_INDEX;
    index_info.size = index_bytes;
    impl.index_buffer = SDL_CreateGPUBuffer(impl.device, &index_info);

    if (impl.vertex_buffer == nullptr || impl.index_buffer == nullptr) {
        return std::unexpected(
            std::format("could not allocate geometry buffers: {}", SDL_GetError()));
    }

    SDL_GPUTransferBufferCreateInfo transfer_info{};
    transfer_info.usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD;
    transfer_info.size = vertex_bytes + index_bytes;
    SDL_GPUTransferBuffer* transfer =
        SDL_CreateGPUTransferBuffer(impl.device, &transfer_info);
    if (transfer == nullptr) {
        return std::unexpected(
            std::format("could not allocate a transfer buffer: {}", SDL_GetError()));
    }

    auto* mapped = static_cast<u8*>(SDL_MapGPUTransferBuffer(impl.device, transfer, false));
    std::memcpy(mapped, vertices.data(), vertex_bytes);
    std::memcpy(mapped + vertex_bytes, indices.data(), index_bytes);
    SDL_UnmapGPUTransferBuffer(impl.device, transfer);

    SDL_GPUCommandBuffer* upload = SDL_AcquireGPUCommandBuffer(impl.device);
    SDL_GPUCopyPass* copy = SDL_BeginGPUCopyPass(upload);

    SDL_GPUTransferBufferLocation source{transfer, 0};
    SDL_GPUBufferRegion destination{impl.vertex_buffer, 0, vertex_bytes};
    SDL_UploadToGPUBuffer(copy, &source, &destination, false);

    source.offset = vertex_bytes;
    destination = SDL_GPUBufferRegion{impl.index_buffer, 0, index_bytes};
    SDL_UploadToGPUBuffer(copy, &source, &destination, false);

    // --- Materials -----------------------------------------------------------

    for (SDL_GPUTexture* texture : impl.textures) {
        SDL_ReleaseGPUTexture(impl.device, texture);
    }
    impl.textures.assign(level.texinfos().size(), nullptr);

    SDL_GPUTransferBufferCreateInfo texture_transfer_info{};
    texture_transfer_info.usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD;
    texture_transfer_info.size = asset::kDevTextureSize * asset::kDevTextureSize * 4;

    for (u32 i = 0; i < level.texinfos().size(); ++i) {
        SDL_GPUTextureCreateInfo texture_info{};
        texture_info.type = SDL_GPU_TEXTURETYPE_2D;
        texture_info.format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM;
        texture_info.usage = SDL_GPU_TEXTUREUSAGE_SAMPLER;
        texture_info.width = asset::kDevTextureSize;
        texture_info.height = asset::kDevTextureSize;
        texture_info.layer_count_or_depth = 1;
        texture_info.num_levels = 1;
        impl.textures[i] = SDL_CreateGPUTexture(impl.device, &texture_info);

        SDL_GPUTransferBuffer* texture_transfer =
            SDL_CreateGPUTransferBuffer(impl.device, &texture_transfer_info);
        const std::vector<u8> pixels =
            asset::dev_texture(level.material_of(level.texinfos()[i]));
        void* texture_mapped =
            SDL_MapGPUTransferBuffer(impl.device, texture_transfer, false);
        std::memcpy(texture_mapped, pixels.data(), pixels.size());
        SDL_UnmapGPUTransferBuffer(impl.device, texture_transfer);

        SDL_GPUTextureTransferInfo texture_source{};
        texture_source.transfer_buffer = texture_transfer;
        SDL_GPUTextureRegion region{};
        region.texture = impl.textures[i];
        region.w = asset::kDevTextureSize;
        region.h = asset::kDevTextureSize;
        region.d = 1;
        SDL_UploadToGPUTexture(copy, &texture_source, &region, false);
        SDL_ReleaseGPUTransferBuffer(impl.device, texture_transfer);
    }

    SDL_EndGPUCopyPass(copy);
    SDL_SubmitGPUCommandBuffer(upload);
    SDL_ReleaseGPUTransferBuffer(impl.device, transfer);

    KERO_INFO(log, "{} vertices, {} triangles, {} materials, {} draw batches",
              vertices.size(), indices.size() / 3, impl.textures.size(),
              impl.batches.size());
    return {};
}

Input Renderer::poll() {
    Input input;
    SDL_Event event;

    while (SDL_PollEvent(&event)) {
        switch (event.type) {
            case SDL_EVENT_QUIT:
                input.quit = true;
                break;
            case SDL_EVENT_MOUSE_MOTION:
                if (mouse_captured_) {
                    // Degrees per pixel. Yaw is negated because moving the
                    // mouse right should turn right, and yaw increases to the
                    // left.
                    input.look_yaw -= event.motion.xrel * 0.15f;
                    input.look_pitch += event.motion.yrel * 0.15f;
                }
                break;
            case SDL_EVENT_MOUSE_BUTTON_DOWN:
                if (!mouse_captured_) {
                    set_mouse_captured(true);
                }
                break;
            case SDL_EVENT_KEY_DOWN:
                if (event.key.key == SDLK_ESCAPE) {
                    // Releases the mouse rather than quitting. A game that
                    // traps the pointer with no way out is one people
                    // force-quit.
                    set_mouse_captured(!mouse_captured_);
                }
                if (event.key.key == SDLK_GRAVE) {
                    input.toggle_console = true;
                }
                break;
            default:
                break;
        }
    }

    const bool* keys = SDL_GetKeyboardState(nullptr);
    if (keys != nullptr) {
        if (keys[SDL_SCANCODE_W]) input.forward += 1.0f;
        if (keys[SDL_SCANCODE_S]) input.forward -= 1.0f;
        if (keys[SDL_SCANCODE_D]) input.side += 1.0f;
        if (keys[SDL_SCANCODE_A]) input.side -= 1.0f;
        input.jump = keys[SDL_SCANCODE_SPACE];
        input.duck = keys[SDL_SCANCODE_LCTRL] || keys[SDL_SCANCODE_RCTRL];
    }

    return input;
}

void Renderer::set_mouse_captured(bool captured) {
    mouse_captured_ = captured;
    SDL_SetWindowRelativeMouseMode(impl_->window, captured);
}

void Renderer::draw(const bsp::Level& level, const Vec3& eye, const Angles& angles,
                    i32 cluster) {
    Impl& impl = *impl_;
    stats_ = Stats{};

    SDL_GPUCommandBuffer* command = SDL_AcquireGPUCommandBuffer(impl.device);
    if (command == nullptr) {
        return;
    }

    SDL_GPUTexture* swapchain = nullptr;
    u32 width = 0;
    u32 height = 0;
    if (!SDL_WaitAndAcquireGPUSwapchainTexture(command, impl.window, &swapchain, &width,
                                               &height) ||
        swapchain == nullptr) {
        // The window is minimised or being resized. Nothing to draw, and the
        // command buffer still has to be submitted or it leaks.
        SDL_SubmitGPUCommandBuffer(command);
        return;
    }

    if (impl.depth_texture == nullptr || impl.depth_width != width ||
        impl.depth_height != height) {
        if (impl.depth_texture != nullptr) {
            SDL_ReleaseGPUTexture(impl.device, impl.depth_texture);
        }
        SDL_GPUTextureCreateInfo depth_info{};
        depth_info.type = SDL_GPU_TEXTURETYPE_2D;
        depth_info.format = impl.depth_format;
        depth_info.usage = SDL_GPU_TEXTUREUSAGE_DEPTH_STENCIL_TARGET;
        depth_info.width = width;
        depth_info.height = height;
        depth_info.layer_count_or_depth = 1;
        depth_info.num_levels = 1;
        impl.depth_texture = SDL_CreateGPUTexture(impl.device, &depth_info);
        impl.depth_width = width;
        impl.depth_height = height;
    }

    const f32 aspect =
        height > 0 ? static_cast<f32>(width) / static_cast<f32>(height) : 1.0f;
    // Near at 1 ku (two inches) and far at four world-widths. The near plane is
    // what depth precision is most sensitive to, so it is as far out as the
    // player's own body allows.
    const math::Mat4 projection = math::Mat4::perspective(75.0f, aspect, 1.0f,
                                                          units::kWorldExtent * 4.0f);
    const math::Mat4 view = math::Mat4::view_from_angles(eye, angles);
    const math::Mat4 view_projection = projection * view;
    const math::Frustum frustum = math::Frustum::from_view_projection(view_projection);

    // --- Cull ---------------------------------------------------------------

    std::ranges::fill(impl.visible, 0);
    level.visible_clusters(cluster, bsp::kVisPvs, impl.pvs);

    for (u32 leaf = 0; leaf < level.leaves().size(); ++leaf) {
        const bsp::DiskLeaf& current = level.leaves()[leaf];
        if (current.cluster < 0) {
            continue;
        }
        // Two cheap tests in the order that discards most: the PVS was computed
        // once at build time and answers "could this ever be seen from there",
        // and the frustum answers "is it on screen now".
        if (!bsp::Level::cluster_in_set(impl.pvs, current.cluster)) {
            continue;
        }
        const math::Aabb bounds(Vec3(current.mins[0], current.mins[1], current.mins[2]),
                                Vec3(current.maxs[0], current.maxs[1], current.maxs[2]));
        if (!frustum.intersects(bounds)) {
            continue;
        }

        ++stats_.leaves_visible;
        for (u32 face : level.leaf_faces(leaf)) {
            const u32 sorted = impl.face_of_source[face];
            if (sorted != Index::kNone) {
                impl.visible[sorted] = 1;
            }
        }
    }

    // --- Draw ---------------------------------------------------------------

    SDL_GPUColorTargetInfo colour{};
    colour.texture = swapchain;
    colour.clear_color = SDL_FColor{0.05f, 0.06f, 0.08f, 1.0f};
    colour.load_op = SDL_GPU_LOADOP_CLEAR;
    colour.store_op = SDL_GPU_STOREOP_STORE;

    SDL_GPUDepthStencilTargetInfo depth{};
    depth.texture = impl.depth_texture;
    depth.clear_depth = 1.0f;
    depth.load_op = SDL_GPU_LOADOP_CLEAR;
    depth.store_op = SDL_GPU_STOREOP_DONT_CARE;

    SDL_GPURenderPass* pass = SDL_BeginGPURenderPass(command, &colour, 1, &depth);
    SDL_BindGPUGraphicsPipeline(pass, impl.pipeline);

    SDL_GPUBufferBinding vertex_binding{impl.vertex_buffer, 0};
    SDL_BindGPUVertexBuffers(pass, 0, &vertex_binding, 1);
    SDL_GPUBufferBinding index_binding{impl.index_buffer, 0};
    SDL_BindGPUIndexBuffer(pass, &index_binding, SDL_GPU_INDEXELEMENTSIZE_32BIT);

    CameraUniform camera{};
    std::memcpy(camera.view_projection, view_projection.data(), sizeof(camera));
    SDL_PushGPUVertexUniformData(command, 0, &camera, sizeof(camera));

    ShadingUniform shading{};
    shading.tint[0] = shading.tint[1] = shading.tint[2] = shading.tint[3] = 1.0f;
    // A key from above and slightly to one side, so floors, walls and ceilings
    // are told apart until Radiance exists.
    shading.key_direction[0] = 0.35f;
    shading.key_direction[1] = 0.25f;
    shading.key_direction[2] = 0.90f;
    SDL_PushGPUFragmentUniformData(command, 0, &shading, sizeof(shading));

    for (const MaterialBatch& batch : impl.batches) {
        SDL_GPUTextureSamplerBinding texture_binding{};
        texture_binding.texture = impl.textures[batch.material];
        texture_binding.sampler = impl.sampler;
        bool bound = false;

        // Adjacent visible faces in a batch are merged into one draw. Sorting
        // by material was what made them adjacent.
        u32 run_first = 0;
        u32 run_count = 0;
        const auto flush = [&] {
            if (run_count == 0) {
                return;
            }
            if (!bound) {
                SDL_BindGPUFragmentSamplers(pass, 0, &texture_binding, 1);
                bound = true;
            }
            SDL_DrawGPUIndexedPrimitives(pass, run_count, 1, run_first, 0, 0);
            ++stats_.draw_calls;
            stats_.triangles += run_count / 3;
            run_count = 0;
        };

        for (u32 i = 0; i < batch.face_count; ++i) {
            const FaceRange& face = impl.faces[batch.first_face + i];
            if (impl.visible[batch.first_face + i] == 0) {
                flush();
                continue;
            }
            ++stats_.faces_drawn;
            if (run_count == 0) {
                run_first = face.first_index;
            }
            run_count += face.index_count;
        }
        flush();
    }

    SDL_EndGPURenderPass(pass);
    SDL_SubmitGPUCommandBuffer(command);
}

}  // namespace kero::render
