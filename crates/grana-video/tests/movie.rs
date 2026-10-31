use grana_video::Recorder;

const SIZE: [u32; 2] = [64, 48];
const FRAMES: u32 = 6;

#[test]
fn writes_an_h264_movie() {
    let path = std::env::temp_dir().join(format!("grana-movie-{}.mp4", std::process::id()));
    let mut recorder = Recorder::start(&path, SIZE, 12).expect("the recorder starts");
    for frame in 0..FRAMES {
        recorder.push(&pixels(frame)).expect("the frame encodes");
    }
    recorder.finish().expect("the movie completes");
    let bytes = std::fs::read(&path).expect("the movie reads");
    std::fs::remove_file(&path).ok();
    assert_eq!(&bytes[4..8], b"ftyp");
    let moov = top_level(&bytes, b"moov").expect("the movie carries a movie header");
    let moov = &bytes[moov.start..moov.end];
    let media = top_level(&bytes, b"mdat").expect("the movie carries media data");
    assert!(media.end - media.start > FRAMES as usize);
    let stsz = find_type(moov, b"stsz").expect("the movie carries sample sizes");
    assert_eq!(be32(&moov[stsz + 12..stsz + 16]), FRAMES);
    let stss = find_type(moov, b"stss").expect("the movie carries sync samples");
    assert_ne!(
        be32(&moov[stss + 8..stss + 12]),
        0,
        "the movie opens on a keyframe"
    );
    assert!(
        find_type(moov, b"avc1").is_some(),
        "the movie carries an h264 track"
    );
    assert!(
        find_type(moov, b"avcC").is_some(),
        "the track carries its parameter sets"
    );
}

struct Span {
    start: usize,
    end: usize,
}

fn top_level(bytes: &[u8], wanted: &[u8; 4]) -> Option<Span> {
    let mut offset = 0;
    while offset + 8 <= bytes.len() {
        let size = be32(&bytes[offset..offset + 4]) as usize;
        if size < 8 || offset + size > bytes.len() {
            return None;
        }
        if &bytes[offset + 4..offset + 8] == wanted {
            return Some(Span {
                start: offset + 8,
                end: offset + size,
            });
        }
        offset += size;
    }
    None
}

fn find_type(bytes: &[u8], kind: &[u8; 4]) -> Option<usize> {
    bytes.windows(4).position(|window| window == kind)
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn pixels(frame: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (SIZE[0] * SIZE[1] * 4) as usize];
    for (index, pixel) in pixels.chunks_mut(4).enumerate() {
        let column = index as u32 % SIZE[0];
        pixel[0] = (column * 3) as u8;
        pixel[1] = 40 + (frame * 8) as u8;
        pixel[2] = 200;
        pixel[3] = 255;
    }
    pixels
}
