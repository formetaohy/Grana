mod common;

use common::{names, scratch};

use grana_video::{Error, Sequence};

#[test]
fn writes_a_named_frame_sequence() {
    let root = scratch("sequence");
    let directory = root.join("frames");
    let mut sequence = Sequence::start(&directory, "cells", 3).expect("the sequence starts");
    assert_eq!(sequence.frames(), 3);
    for (index, time) in [0u32, 2, 5].into_iter().enumerate() {
        sequence.push(time, b"frame").expect("the frame writes");
        assert_eq!(sequence.frame(), index as u32 + 1);
    }
    assert!(sequence.complete(), "every frame was written");
    let path = sequence.finish().expect("the sequence finishes");
    assert_eq!(path, directory);
    assert_eq!(
        names(&directory),
        [
            "cells-0000-t0000.png",
            "cells-0001-t0002.png",
            "cells-0002-t0005.png"
        ]
    );
    assert_eq!(names(&root), ["frames"], "the staging folder is gone");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rejects_an_existing_folder_and_cleans_an_abandoned_sequence() {
    let root = scratch("sequence-fail");
    let directory = root.join("frames");
    std::fs::create_dir(&directory).expect("the folder is created");
    std::fs::write(directory.join("kept.txt"), b"kept").expect("the sentinel writes");
    assert!(matches!(
        Sequence::start(&directory, "cells", 2),
        Err(Error::Exists { .. })
    ));
    assert_eq!(names(&directory), ["kept.txt"], "the folder is untouched");
    let abandoned = root.join("abandoned");
    {
        let mut sequence = Sequence::start(&abandoned, "cells", 2).expect("the sequence starts");
        sequence.push(0, b"frame").expect("the frame writes");
    }
    assert!(
        !abandoned.exists(),
        "an abandoned sequence leaves no folder"
    );
    assert_eq!(
        names(&root),
        ["frames"],
        "an abandoned sequence leaves no staging folder"
    );
    std::fs::remove_dir_all(&root).ok();
}
