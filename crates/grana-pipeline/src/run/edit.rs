use std::sync::Arc;

use grana_object::{SurfaceEdit, SurfaceSet};
use grana_scene::{ObjectFrame, ObjectSource};
use grana_store::SpillStore;

pub fn edit(
    source: &ObjectSource,
    edits: &[SurfaceEdit],
    voxel: f32,
    store: Option<&Arc<SpillStore>>,
) -> Result<Vec<crate::run::EditedFrame>, String> {
    let mut edited = Vec::new();
    for (frame, edits) in grouped(edits) {
        if frame >= source.frames() {
            return Err(format!(
                "the surface edits name frame {frame}, which the object set does not hold"
            ));
        }
        let Some(current) = source.frame(frame) else {
            return Err(format!(
                "the surface edits name frame {frame}, which detected no objects"
            ));
        };
        let ObjectFrame::Surfaces(set) = current.as_ref() else {
            return Err("only surfaces can be merged or split".to_string());
        };
        let mut next: Option<SurfaceSet> = None;
        for edit in edits {
            let base = next.as_ref().unwrap_or(set);
            next = Some(grana_analysis::apply(base, edit, voxel, store)?);
        }
        edited.push((frame, next.map(|set| Arc::new(ObjectFrame::Surfaces(set)))));
    }
    Ok(edited)
}

fn grouped(edits: &[SurfaceEdit]) -> Vec<(u32, Vec<&SurfaceEdit>)> {
    let mut frames: Vec<(u32, Vec<&SurfaceEdit>)> = Vec::new();
    for edit in edits {
        match frames.iter_mut().find(|(frame, _)| *frame == edit.frame()) {
            Some((_, held)) => held.push(edit),
            None => frames.push((edit.frame(), vec![edit])),
        }
    }
    frames
}
