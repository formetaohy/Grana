use crate::support::Temp;

const FRAMES: u8 = 4;

struct Atom<'a> {
    kind: [u8; 4],
    start: usize,
    body: &'a [u8],
}

#[test]
fn writes_chunked_media_boxes_with_matching_tables() {
    let temp = Temp::new("grana-movie");
    for (name, size, alpha) in [
        ("movie.mp4", [96, 64], false),
        ("movie.mov", [16, 16], true),
    ] {
        let path = temp.path().join(name);
        let mut recorder =
            grana_video::Recorder::start(&path, size, 25, alpha).expect("the recorder opens");
        for frame in 0..FRAMES {
            recorder
                .push(rgba(size, frame))
                .expect("the frame is recorded");
        }
        recorder.finish().expect("the movie is written");
        let bytes = std::fs::read(&path).expect("the movie lands on disk");
        verify(&bytes, alpha);
    }
}

fn verify(bytes: &[u8], alpha: bool) {
    let atoms = atoms(bytes);
    assert_eq!(
        atoms.first().map(|atom| atom.kind),
        Some(*b"ftyp"),
        "the movie opens with its brand"
    );
    let brand = if alpha { *b"qt  " } else { *b"isom" };
    assert_eq!(
        &atoms[0].body[..4],
        brand.as_slice(),
        "the movie keeps its brand"
    );
    let moov = atoms.last().expect("the movie carries its metadata");
    assert_eq!(moov.kind, *b"moov", "the metadata box closes the movie");
    assert_eq!(
        moov.start + 8 + moov.body.len(),
        bytes.len(),
        "the metadata box ends the file"
    );

    let stbl = descend(moov.body, &[*b"trak", *b"mdia", *b"minf", *b"stbl"]);
    let stsd = descend(stbl, &[*b"stsd"]);
    let entry = if alpha { *b"png " } else { *b"avc1" };
    assert_eq!(
        value(stsd, 4),
        1,
        "the movie describes a single sample format"
    );
    assert_eq!(
        &stsd[12..16],
        entry.as_slice(),
        "the movie keeps its sample format"
    );

    let sizes = sample_sizes(descend(stbl, &[*b"stsz"]));
    assert_eq!(
        sizes.len(),
        usize::from(FRAMES),
        "the movie holds every recorded sample"
    );
    let offsets = chunk_offsets(stbl);
    let media: Vec<&Atom<'_>> = atoms.iter().filter(|atom| atom.kind == *b"mdat").collect();
    assert!(!media.is_empty(), "the movie carries its media");
    assert_eq!(
        offsets.len(),
        media.len(),
        "every media box is a chunk with its offset"
    );
    let chunks = chunk_samples(descend(stbl, &[*b"stsc"]), media.len());
    assert_eq!(
        chunks.iter().map(|count| *count as usize).sum::<usize>(),
        sizes.len(),
        "the chunks cover every sample"
    );

    let mut sample = 0;
    for (chunk, atom) in media.iter().enumerate() {
        let count = chunks[chunk] as usize;
        let payload = sizes[sample..sample + count]
            .iter()
            .map(|size| u64::from(*size))
            .sum::<u64>();
        sample += count;
        assert_eq!(
            atom.body.len() as u64,
            payload,
            "a media box holds exactly the samples of its chunk"
        );
        assert_eq!(
            offsets[chunk],
            (atom.start + 8) as u64,
            "a chunk offset points at its media payload"
        );
    }
    assert_eq!(sample, sizes.len(), "the media boxes cover every sample");
}

fn rgba(size: [u32; 2], frame: u8) -> Vec<u8> {
    let width = size[0] as usize;
    let mut bytes = vec![0u8; width * size[1] as usize * 4];
    for (index, pixel) in bytes.chunks_mut(4).enumerate() {
        pixel[0] = (index % width) as u8;
        pixel[1] = (index / width) as u8;
        pixel[2] = frame.wrapping_mul(16);
        pixel[3] = 255;
    }
    bytes
}

fn atoms(bytes: &[u8]) -> Vec<Atom<'_>> {
    let mut found = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        assert!(bytes.len() - start >= 8, "a box header fits its parent");
        let size = u32::from_be_bytes(
            bytes[start..start + 4]
                .try_into()
                .expect("a box size is four bytes"),
        ) as usize;
        assert!(
            size >= 8 && start + size <= bytes.len(),
            "a box spans its parent"
        );
        let kind: [u8; 4] = bytes[start + 4..start + 8]
            .try_into()
            .expect("a box type is four bytes");
        found.push(Atom {
            kind,
            start,
            body: &bytes[start + 8..start + size],
        });
        start += size;
    }
    found
}

fn descend<'a>(body: &'a [u8], kinds: &[[u8; 4]]) -> &'a [u8] {
    let mut current = body;
    for kind in kinds {
        current = atoms(current)
            .into_iter()
            .find(|atom| atom.kind == *kind)
            .map(|atom| atom.body)
            .expect("a movie box holds its children");
    }
    current
}

fn value(body: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(
        body[at..at + 4]
            .try_into()
            .expect("a table value is four bytes"),
    )
}

fn wide_value(body: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(
        body[at..at + 8]
            .try_into()
            .expect("a wide table value is eight bytes"),
    )
}

fn sample_sizes(body: &[u8]) -> Vec<u32> {
    let body = &body[4..];
    assert_eq!(
        value(body, 0),
        0,
        "the sample sizes are recorded one by one"
    );
    let count = value(body, 4) as usize;
    assert_eq!(
        body.len(),
        8 + count * 4,
        "the sample size table holds its entries"
    );
    (0..count).map(|index| value(body, 8 + index * 4)).collect()
}

fn chunk_samples(body: &[u8], chunks: usize) -> Vec<u32> {
    let body = &body[4..];
    let entries = value(body, 0) as usize;
    assert_eq!(
        body.len(),
        4 + entries * 12,
        "the sample to chunk table holds its entries"
    );
    let runs: Vec<(u32, u32)> = (0..entries)
        .map(|index| (value(body, 4 + index * 12), value(body, 8 + index * 12)))
        .collect();
    if let Some(run) = runs.first() {
        assert_eq!(
            run.0, 1,
            "the sample to chunk runs start at the first chunk"
        );
    }
    let mut samples = Vec::with_capacity(chunks);
    let mut run = 0;
    for chunk in 1..=chunks as u32 {
        while run + 1 < runs.len() && runs[run + 1].0 <= chunk {
            run += 1;
        }
        samples.push(
            runs.get(run)
                .expect("every chunk belongs to a sample to chunk run")
                .1,
        );
    }
    samples
}

fn chunk_offsets(stbl: &[u8]) -> Vec<u64> {
    let atoms = atoms(stbl);
    let narrow = atoms.iter().find(|atom| atom.kind == *b"stco");
    let wide = atoms.iter().find(|atom| atom.kind == *b"co64");
    if let Some(narrow) = narrow {
        assert!(wide.is_none(), "the movie carries one chunk offset table");
        let body = &narrow.body[4..];
        let count = value(body, 0) as usize;
        assert_eq!(
            body.len(),
            4 + count * 4,
            "the chunk offset table holds its entries"
        );
        (0..count)
            .map(|index| u64::from(value(body, 4 + index * 4)))
            .collect()
    } else {
        let wide = wide.expect("the movie carries a chunk offset table");
        let body = &wide.body[4..];
        let count = value(body, 0) as usize;
        assert_eq!(
            body.len(),
            4 + count * 8,
            "the chunk offset table holds its entries"
        );
        (0..count)
            .map(|index| wide_value(body, 4 + index * 8))
            .collect()
    }
}
