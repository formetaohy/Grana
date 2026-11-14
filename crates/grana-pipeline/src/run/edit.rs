use std::sync::Arc;

use grana_object::SurfaceEdit;
use grana_scene::ObjectFrame;
use grana_store::SpillStore;

pub fn edit(
    frames: &[Option<Arc<ObjectFrame>>],
    edits: &[SurfaceEdit],
    voxel: f32,
    store: Option<&Arc<SpillStore>>,
) -> Result<Vec<Option<Arc<ObjectFrame>>>, String> {
    let mut edited = frames.to_vec();
    for edit in edits {
        let frame = edit.frame();
        let Some(entry) = edited.get_mut(frame as usize) else {
            return Err(format!(
                "the surface edits name frame {frame}, which the object set does not hold"
            ));
        };
        let Some(current) = entry.as_deref() else {
            return Err(format!(
                "the surface edits name frame {frame}, which detected no objects"
            ));
        };
        let ObjectFrame::Surfaces(set) = current else {
            return Err("only surfaces can be merged or split".to_string());
        };
        let next = grana_analysis::apply(set, edit, voxel, store)?;
        *entry = Some(Arc::new(ObjectFrame::Surfaces(next)));
    }
    Ok(edited)
}
