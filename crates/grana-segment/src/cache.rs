use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::Result;
use crate::model::Model;

struct Resident {
    path: PathBuf,
    model: Arc<Model>,
}

static RESIDENT: OnceLock<Mutex<Option<Resident>>> = OnceLock::new();

pub(crate) fn acquire(path: &Path) -> Result<Arc<Model>> {
    let resident = RESIDENT.get_or_init(|| Mutex::new(None));
    let mut guard = resident.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(resident) = guard.as_ref()
        && resident.path == path
    {
        return Ok(Arc::clone(&resident.model));
    }
    let model = Arc::new(Model::open(path)?);
    *guard = Some(Resident {
        path: path.to_path_buf(),
        model: Arc::clone(&model),
    });
    Ok(model)
}
