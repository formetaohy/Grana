use std::path::PathBuf;

use crate::plan::{Destination, Plan};

pub(crate) fn write(plan: &Plan) -> Option<Result<PathBuf, String>> {
    let Destination::Chart { path, page } = &plan.destination else {
        return None;
    };
    Some((|| {
        crate::plan::validate(plan)?;
        grana_page::write_page(page, path)
    })())
}
