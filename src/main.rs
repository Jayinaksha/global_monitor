use eframe::egui;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::runtime::Runtime;

mod data;
mod gpu_renderer;

use data::fetch_aircraft;
use gpu_renderer::{AircraftCallback, AircraftGpuResources};

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Global Monitor",
        options,
        Box::new(|cc| Box::new(GlobalApp::new(cc))),
    )
}

struct GlobalApp {
    aircraft: Vec<(f32, f32)>,
    aircraft_shared: Arc<Mutex<Vec<(f32, f32)>>>,
    #[allow(dead_code)]
    runtime: Runtime,
    map_texture: Option<egui::TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
    trails: HashMap<usize, Vec<(f32, f32)>>,
    #[allow(dead_code)]
    last_update: std::time::Instant,
    gpu_initialized: bool,
}

impl GlobalApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        let aircraft_shared = Arc::new(Mutex::new(Vec::new()));
        let shared_clone = aircraft_shared.clone();
        let runtime_clone = runtime.handle().clone();

        // Background thread: fetch aircraft data every 5 seconds
        thread::spawn(move || {
            loop {
                let aircraft = runtime_clone.block_on(fetch_aircraft());
                if let Ok(mut data) = shared_clone.lock() {
                    *data = aircraft;
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });

        // Initialize GPU resources into eframe's callback_resources
        let gpu_initialized = if let Some(render_state) = &cc.wgpu_render_state {
            let device = &render_state.device;
            let format = render_state.target_format;
            let resources = AircraftGpuResources::new(device, format);
            render_state
                .renderer
                .write()
                .callback_resources
                .insert(resources);
            true
        } else {
            eprintln!("Warning: wgpu render state not available — GPU rendering disabled");
            false
        };

        Self {
            aircraft: Vec::new(),
            aircraft_shared,
            runtime,
            map_texture: None,
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
            trails: HashMap::new(),
            last_update: std::time::Instant::now(),
            gpu_initialized,
        }
    }

    fn world_to_screen(&self, rect: egui::Rect, lat: f32, lon: f32) -> egui::Pos2 {
        let x = rect.left() + ((lon + 180.0) / 360.0 * rect.width()) * self.zoom + self.offset.x;
        let y = rect.top() + ((90.0 - lat) / 180.0 * rect.height()) * self.zoom + self.offset.y;
        egui::pos2(x, y)
    }
}

impl eframe::App for GlobalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());

        // ---------- LOAD MAP TEXTURE (once) ----------
        if self.map_texture.is_none() {
            let image_bytes = match std::fs::read("src/assets/world_map.png") {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!("Failed to load map image: {e}");
                    return;
                }
            };

            let image = match image::load_from_memory(&image_bytes) {
                Ok(img) => img.to_rgba8(),
                Err(e) => {
                    eprintln!("Failed to decode map image: {e}");
                    return;
                }
            };

            let size = [image.width() as _, image.height() as _];
            let texture = ctx.load_texture(
                "world_map",
                egui::ColorImage::from_rgba_unmultiplied(size, &image),
                Default::default(),
            );
            self.map_texture = Some(texture);
        }

        // ---------- COPY SHARED AIRCRAFT ----------
        if let Ok(data) = self.aircraft_shared.lock() {
            self.aircraft = data.clone();
        }

        // ---------- UPDATE TRAILS ----------
        for (i, (lat, lon)) in self.aircraft.iter().enumerate() {
            let trail = self.trails.entry(i).or_default();
            trail.push((*lat, *lon));
            if trail.len() > 6 {
                trail.remove(0);
            }
        }

        // ---------- ZOOM (mouse wheel) ----------
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let zoom_factor = 1.0 + scroll * 0.001;
            let mouse = ctx
                .input(|i| i.pointer.hover_pos())
                .unwrap_or(egui::pos2(0.0, 0.0));
            let before = (mouse.to_vec2() - self.offset) / self.zoom;
            self.zoom *= zoom_factor;
            self.zoom = self.zoom.clamp(0.2, 8.0);
            let after = before * self.zoom + self.offset;
            self.offset += mouse.to_vec2() - after;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Global Situational Viewer");

            let rect = ui.available_rect_before_wrap();
            let painter = ui.painter();

            // ---------- PAN (drag) ----------
            let response = ui.interact(rect, ui.id().with("map"), egui::Sense::drag());
            if response.dragged() {
                self.offset += response.drag_delta();
            }

            // ---------- DRAW MAP (via egui texture — lightweight) ----------
            if let Some(texture) = &self.map_texture {
                let map_rect =
                    egui::Rect::from_min_size(rect.min + self.offset, rect.size() * self.zoom);
                painter.image(
                    texture.id(),
                    map_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }

            // ---------- GPU-ACCELERATED AIRCRAFT + TRAILS ----------
            if self.gpu_initialized {
                // Build aircraft position array for the GPU
                let aircraft_positions: Vec<[f32; 2]> = self
                    .aircraft
                    .iter()
                    .filter(|(lat, lon)| {
                        // Frustum cull: skip aircraft clearly off-screen
                        let pos = self.world_to_screen(rect, *lat, *lon);
                        rect.expand(30.0).contains(pos)
                    })
                    .map(|(lat, lon)| [*lat, *lon])
                    .collect();

                // Build trail line segments for the GPU
                let mut trail_vertices: Vec<[f32; 2]> = Vec::new();
                for trail in self.trails.values() {
                    for w in trail.windows(2) {
                        let (lat1, lon1) = w[0];
                        let (lat2, lon2) = w[1];
                        // Frustum cull trail segments
                        let p1 = self.world_to_screen(rect, lat1, lon1);
                        let p2 = self.world_to_screen(rect, lat2, lon2);
                        if rect.expand(30.0).contains(p1) || rect.expand(30.0).contains(p2) {
                            trail_vertices.push([lat1, lon1]);
                            trail_vertices.push([lat2, lon2]);
                        }
                    }
                }

                let ppp = ctx.pixels_per_point();
                let viewport_px = [rect.width() * ppp, rect.height() * ppp];

                let callback = AircraftCallback {
                    aircraft_positions: Arc::new(aircraft_positions),
                    trail_vertices: Arc::new(trail_vertices),
                    rect: [rect.left(), rect.top(), rect.width(), rect.height()],
                    viewport_px,
                    zoom: self.zoom,
                    offset: [self.offset.x, self.offset.y],
                    point_size: 5.0,
                };

                let paint_callback = egui_wgpu::Callback::new_paint_callback(rect, callback);

                painter.add(egui::Shape::Callback(paint_callback));
            } else {
                // Fallback: CPU rendering (same as original code, for systems without wgpu)
                self.draw_cpu_fallback(painter, rect);
            }
        });

        // Request continuous repaint for smooth animation
        ctx.request_repaint();
    }
}

// --- CPU fallback rendering (used only when wgpu is not available) ---
impl GlobalApp {
    fn draw_cpu_fallback(&self, painter: &egui::Painter, rect: egui::Rect) {
        // Draw trails
        for trail in self.trails.values() {
            for w in trail.windows(2) {
                let (lat1, lon1) = w[0];
                let (lat2, lon2) = w[1];
                let p1 = self.world_to_screen(rect, lat1, lon1);
                let p2 = self.world_to_screen(rect, lat2, lon2);
                painter.line_segment([p1, p2], egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE));
            }
        }

        // Draw aircraft
        let max_planes = 1500;
        for (lat, lon) in self.aircraft.iter().take(max_planes) {
            let pos = self.world_to_screen(rect, *lat, *lon);
            if !rect.expand(20.0).contains(pos) {
                continue;
            }
            painter.circle_filled(
                pos,
                5.0,
                egui::Color32::from_rgba_unmultiplied(80, 160, 255, 40),
            );
            painter.circle_filled(pos, 2.5, egui::Color32::LIGHT_BLUE);
        }
    }
}
