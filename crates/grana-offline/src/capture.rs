use grana_display::{linear_to_srgb, srgb_component_to_linear};
use wgpu::{
    CommandEncoder, Device, Extent3d, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView,
};

pub(crate) enum Capture {
    Png(Vec<u8>),
    Tiff(Vec<u16>),
}

pub(crate) fn texture(
    device: &Device,
    label: &'static str,
    format: TextureFormat,
    size: [u32; 2],
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

pub(crate) fn clear(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.export.overlay.clear"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

pub(crate) fn compose(
    picture: &grana_render::Image16,
    overlay: &grana_render::Image,
    transparent: bool,
) -> Vec<u16> {
    assert_eq!(
        picture.size(),
        overlay.size(),
        "the figure layer covers its scene"
    );
    let mut composed = Vec::with_capacity(picture.pixels().len());
    for (below, above) in picture
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(overlay.pixels().as_chunks::<4>().0)
    {
        if transparent {
            over_transparent(below, above, &mut composed);
        } else {
            over_opaque(below, above, &mut composed);
        }
    }
    composed
}

fn over_opaque(below: &[u16], above: &[u8], out: &mut Vec<u16>) {
    let keep = 1.0 - f32::from(above[3]) / 255.0;
    for (index, value) in below.iter().take(3).enumerate() {
        let mixed = f32::from(above[index]) * 257.0 + f32::from(*value) * keep;
        out.push(mixed.round().clamp(0.0, 65535.0) as u16);
    }
    out.push(u16::MAX);
}

fn over_transparent(below: &[u16], above: &[u8], out: &mut Vec<u16>) {
    let front = f32::from(above[3]) / 255.0;
    let back = f32::from(below[3]) / 65535.0;
    let covered = back * (1.0 - front);
    let alpha = front + covered;
    if alpha <= 0.0 {
        out.extend_from_slice(&[0; 4]);
        return;
    }
    for index in 0..3 {
        let premultiplied = if front > 0.0 {
            srgb_component_to_linear(f32::from(above[index]) / 255.0 / front) * front
        } else {
            0.0
        };
        let value = (premultiplied
            + srgb_component_to_linear(f32::from(below[index]) / 65535.0) * covered)
            / alpha;
        out.push(quantize(linear_to_srgb(value)));
    }
    out.push((alpha * 65535.0).round().clamp(0.0, 65535.0) as u16);
}

fn quantize(encoded: f32) -> u16 {
    (encoded.clamp(0.0, 1.0) * 65535.0).round() as u16
}
