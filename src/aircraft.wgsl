// --- Uniforms ---
// rect:     (left, top, width, height) in points
// viewport: (vp_width_px, vp_height_px, zoom, _)
// offset:   (offset_x, offset_y, point_size, _)

struct Uniforms {
    rect:     vec4<f32>,
    viewport: vec4<f32>,
    offset:   vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// --- Helper: lat/lon to clip space ---
fn latlon_to_clip(lat: f32, lon: f32) -> vec2<f32> {
    let rect_left  = u.rect.x;
    let rect_top   = u.rect.y;
    let rect_w     = u.rect.z;
    let rect_h     = u.rect.w;
    let zoom       = u.viewport.z;
    let off_x      = u.offset.x;
    let off_y      = u.offset.y;
    let vp_w       = u.viewport.x;
    let vp_h       = u.viewport.y;

    // World-to-screen (same math as Rust world_to_screen)
    let sx = rect_left + ((lon + 180.0) / 360.0 * rect_w) * zoom + off_x;
    let sy = rect_top  + ((90.0 - lat) / 180.0 * rect_h) * zoom + off_y;

    // Screen points to NDC clip space: [-1, 1]
    let cx = sx / vp_w * 2.0 - 1.0;
    let cy = 1.0 - sy / vp_h * 2.0;

    return vec2<f32>(cx, cy);
}

// --- AIRCRAFT VERTEX SHADER ---
// Instanced: 6 vertices per quad (two triangles), one instance per aircraft.
// Instance data: vec2<f32> = (lat, lon) at location 0.

struct AircraftVsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_aircraft(
    @builtin(vertex_index) vid: u32,
    @location(0) latlon: vec2<f32>,
) -> AircraftVsOut {
    let center = latlon_to_clip(latlon.x, latlon.y);

    let point_size = u.offset.z;
    let vp_w = u.viewport.x;
    let vp_h = u.viewport.y;

    // Pixel size to clip-space offset
    let dx = point_size / vp_w * 2.0;
    let dy = point_size / vp_h * 2.0;

    // 6 vertices forming a quad: two triangles
    // 0--1
    // | /|
    // |/ |
    // 2--3
    // Tri 1: 0,2,1   Tri 2: 1,2,3
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0,  1.0),  // 0 top-left
        vec2<f32>( 1.0,  1.0),  // 1 top-right
        vec2<f32>(-1.0, -1.0),  // 2 bottom-left
        vec2<f32>( 1.0,  1.0),  // 1 top-right
        vec2<f32>(-1.0, -1.0),  // 2 bottom-left
        vec2<f32>( 1.0, -1.0),  // 3 bottom-right
    );

    let corner = corners[vid];
    let clip_pos = vec2<f32>(
        center.x + corner.x * dx,
        center.y + corner.y * dy,
    );

    var out: AircraftVsOut;
    out.pos = vec4<f32>(clip_pos, 0.0, 1.0);
    out.uv = corner; // [-1,1] range for SDF circle
    return out;
}

@fragment
fn fs_aircraft(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let dist = length(uv);

    // Core dot: solid light blue
    let core_color = vec4<f32>(0.53, 0.81, 0.98, 1.0);
    // Glow: soft blue halo
    let glow_color = vec4<f32>(0.31, 0.63, 1.0, 0.15);

    if dist > 1.0 {
        discard;
    }

    // Inner core (r < 0.5) = solid, outer ring = glow
    let core_alpha = smoothstep(0.55, 0.45, dist);
    let glow_alpha = smoothstep(1.0, 0.3, dist) * 0.15;

    let color = mix(glow_color, core_color, core_alpha);
    let alpha = max(core_alpha, glow_alpha);
    return vec4<f32>(color.rgb, alpha);
}

// --- TRAIL VERTEX SHADER ---
// Simple pass-through: each vertex is a (lat, lon) pair.

@vertex
fn vs_trail(
    @location(0) latlon: vec2<f32>,
) -> @builtin(position) vec4<f32> {
    let clip = latlon_to_clip(latlon.x, latlon.y);
    return vec4<f32>(clip, 0.0, 1.0);
}

@fragment
fn fs_trail() -> @location(0) vec4<f32> {
    return vec4<f32>(0.53, 0.81, 0.92, 0.6);
}
