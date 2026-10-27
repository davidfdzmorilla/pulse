//! Pictures of the interface without a window: egui's triangles drawn in
//! memory, as the GPU would, and saved as PNG. For looking at a layout from a
//! test (`PULSE_SHOTS=dir cargo test --release -- --ignored shots`).

use eframe::egui::{self, epaint, Color32, TextureId};
use std::collections::HashMap;
use std::io::Write;

/// The textures a frame draws with (the font atlas, images), kept across frames.
#[derive(Default)]
pub struct Canvas {
    textures: HashMap<TextureId, ([usize; 2], Vec<Color32>)>,
}

impl Canvas {
    /// Draws one frame's output, `size` points at `scale` pixels per point.
    pub fn draw(&mut self, ctx: &egui::Context, output: egui::FullOutput, size: egui::Vec2, scale: f32) -> Image {
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                let epaint::ImageData::Color(image) = &delta.image;
                match delta.pos {
                    None => {
                        self.textures.insert(*id, (image.size, image.pixels.clone()));
                    }
                    Some([x0, y0]) => {
                        let Some((size, pixels)) = self.textures.get_mut(id) else { continue };
                        for y in 0..image.size[1] {
                            for x in 0..image.size[0] {
                                pixels[(y0 + y) * size[0] + x0 + x] = image.pixels[y * image.size[0] + x];
                            }
                        }
                    }
                }
            }
        }
        let width = (size.x * scale).round() as usize;
        let height = (size.y * scale).round() as usize;
        let mut image = Image { width, height, pixels: vec![Color32::BLACK; width * height] };
        for primitive in ctx.tessellate(output.shapes, scale) {
            let epaint::Primitive::Mesh(mesh) = &primitive.primitive else { continue };
            let Some((tex_size, texture)) = self.textures.get(&mesh.texture_id) else { continue };
            let clip = primitive.clip_rect;
            let clip = [clip.min.x * scale, clip.min.y * scale, clip.max.x * scale, clip.max.y * scale];
            for triangle in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [0, 1, 2].map(|i| &mesh.vertices[triangle[i] as usize]);
                image.triangle([a, b, c], scale, clip, *tex_size, texture);
            }
        }
        for id in &output.textures_delta.free {
            self.textures.remove(id);
        }
        image
    }
}

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Color32>,
}

fn sample(size: [usize; 2], texture: &[Color32], u: f32, v: f32) -> [f32; 4] {
    // Bilinear, as the GPU samples the font atlas.
    let x = (u * size[0] as f32 - 0.5).max(0.0);
    let y = (v * size[1] as f32 - 0.5).max(0.0);
    let (x0, y0) = ((x as usize).min(size[0] - 1), (y as usize).min(size[1] - 1));
    let (x1, y1) = ((x0 + 1).min(size[0] - 1), (y0 + 1).min(size[1] - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| texture[y * size[0] + x].to_array().map(|c| c as f32);
    let (p00, p10, p01, p11) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    std::array::from_fn(|i| (p00[i] * (1.0 - fx) + p10[i] * fx) * (1.0 - fy) + (p01[i] * (1.0 - fx) + p11[i] * fx) * fy)
}

impl Image {
    fn triangle(&mut self, v: [&epaint::Vertex; 3], scale: f32, clip: [f32; 4], tex_size: [usize; 2], texture: &[Color32]) {
        let p = v.map(|v| (v.pos.x * scale, v.pos.y * scale));
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1e-9 {
            return;
        }
        let min_x = p.iter().map(|p| p.0).fold(f32::MAX, f32::min).max(clip[0]).max(0.0).floor() as usize;
        let max_x = p.iter().map(|p| p.0).fold(f32::MIN, f32::max).min(clip[2]).min(self.width as f32).ceil() as usize;
        let min_y = p.iter().map(|p| p.1).fold(f32::MAX, f32::min).max(clip[1]).max(0.0).floor() as usize;
        let max_y = p.iter().map(|p| p.1).fold(f32::MIN, f32::max).min(clip[3]).min(self.height as f32).ceil() as usize;
        let colors = v.map(|v| v.color.to_array().map(|c| c as f32));
        for y in min_y..max_y {
            for x in min_x..max_x {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                let tex = sample(tex_size, texture, u, t);
                // Premultiplied: color × texture, then over what is there.
                let src: [f32; 4] = std::array::from_fn(|i| (w0 * colors[0][i] + w1 * colors[1][i] + w2 * colors[2][i]) * tex[i] / 255.0);
                let dst = &mut self.pixels[y * self.width + x];
                let d = dst.to_array().map(|c| c as f32);
                let out: [u8; 4] = std::array::from_fn(|i| (src[i] + d[i] * (1.0 - src[3] / 255.0)).round().clamp(0.0, 255.0) as u8);
                *dst = Color32::from_rgba_premultiplied(out[0], out[1], out[2], 255);
            }
        }
    }

    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut raw = Vec::with_capacity((self.width * 3 + 1) * self.height);
        for row in self.pixels.chunks_exact(self.width) {
            raw.push(0);
            for c in row {
                raw.extend_from_slice(&[c.r(), c.g(), c.b()]);
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&raw)?;
        let data = z.finish()?;
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&(self.width as u32).to_be_bytes());
        header.extend_from_slice(&(self.height as u32).to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        for (kind, body) in [(b"IHDR", header), (b"IDAT", data), (b"IEND", Vec::new())] {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let mut chunk = kind.to_vec();
            chunk.extend_from_slice(&body);
            out.extend_from_slice(&chunk);
            out.extend_from_slice(&crc32(&chunk).to_be_bytes());
        }
        std::fs::write(path, out)
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
