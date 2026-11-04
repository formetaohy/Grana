use std::io::BufReader;

use grana_video::Animation;

const SIZE: [u32; 2] = [48, 32];
const FRAMES: u32 = 3;
const FPS: u32 = 12;

#[test]
fn writes_a_transparent_animation() {
    let path = std::env::temp_dir().join(format!("grana-animation-{}.png", std::process::id()));
    let mut animation = Animation::start(&path, SIZE, FRAMES, FPS).expect("the animation starts");
    for frame in 0..FRAMES {
        animation.push(&pixels(frame)).expect("the frame encodes");
    }
    animation.finish().expect("the animation completes");
    let file = std::fs::File::open(&path).expect("the animation reads");
    let mut reader = png::Decoder::new(BufReader::new(file))
        .read_info()
        .expect("the animation header reads");
    let control = reader
        .info()
        .animation_control()
        .expect("the animation carries its control");
    assert_eq!(control.num_frames, FRAMES, "the animation holds its frames");
    let mut buffer = vec![0; reader.output_buffer_size().expect("the frames are sized")];
    for frame in 0..FRAMES {
        let info = reader.next_frame(&mut buffer).expect("the frame decodes");
        assert_eq!([info.width, info.height], SIZE);
        assert_eq!(
            info.color_type,
            png::ColorType::Rgba,
            "the frames carry an alpha channel"
        );
        let control = reader
            .info()
            .frame_control()
            .expect("the frame carries its delay");
        assert_eq!(
            [u32::from(control.delay_num), u32::from(control.delay_den)],
            [1, FPS],
            "the frame holds its delay"
        );
        assert_eq!(buffer[3], 0, "the background keeps its transparency");
        assert_eq!(buffer[4 * (SIZE[0] * SIZE[1] / 2) as usize + 3], 255);
        assert_eq!(
            buffer[4 * (SIZE[0] * SIZE[1] / 2) as usize + 1],
            40 + (frame * 8) as u8,
            "the frame keeps its own colour"
        );
    }
    std::fs::remove_file(&path).ok();
}

fn pixels(frame: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (SIZE[0] * SIZE[1] * 4) as usize];
    for (index, pixel) in pixels.chunks_mut(4).enumerate() {
        let centre = index as u32 >= SIZE[0] * SIZE[1] / 2;
        pixel[0] = 20;
        pixel[1] = 40 + (frame * 8) as u8;
        pixel[2] = 200;
        pixel[3] = if centre { 255 } else { 0 };
    }
    pixels
}
