use semver::Version;

use crate::Error;
use crate::manifest::Offer;
use crate::source::Source;

pub fn check(source: &Source, current: &Version) -> Result<Option<Offer>, Error> {
    let url = source.manifest();
    let fetched = grana_net::Client::shared().get(&url, None)?;
    let Some(fetched) = fetched else {
        return Ok(None);
    };
    let text = String::from_utf8(fetched.bytes).map_err(|_| Error::Manifest {
        url,
        detail: "the manifest is not utf-8".to_string(),
    })?;
    let offer = Offer::read(&text, source)?;
    Ok((offer.version > *current).then_some(offer))
}
