#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct Cursor {
    pub(super) left: Id,
    pub(super) right: Id,
    pub(super) frames: Vec<Frame>,
    pub(super) results_after: Option<Vec<u8>>,
    pub(super) done: bool,
}

impl Serialize for Cursor {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let after = self
            .frames
            .last()
            .and_then(|frame| frame.after.clone())
            .map(String::from_utf8)
            .transpose()
            .map_err(S::Error::custom)?;
        let results_after = self
            .results_after
            .clone()
            .map(String::from_utf8)
            .transpose()
            .map_err(S::Error::custom)?;
        CompactCursor {
            left: self.left.to_string(),
            right: self.right.to_string(),
            path: (!self.frames.is_empty()).then(|| {
                self.frames
                    .iter()
                    .skip(1)
                    .map(|frame| frame.name.as_str())
                    .collect::<Vec<_>>()
                    .join("/")
            }),
            emitted: self.frames.last().is_some_and(|frame| frame.emitted),
            after,
            results_after,
            done: self.done,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let wire = CompactCursor::deserialize(deserializer)?;
        let mut frames = Vec::new();
        if let Some(path) = wire.path {
            if path.len() > 4096 {
                return Err(D::Error::custom("diff path too long"));
            }
            let mut names = vec![String::new()];
            if !path.is_empty() {
                names.extend(path.split('/').map(str::to_owned));
            }
            for (index, name) in names.iter().enumerate() {
                frames.push(Frame {
                    name: name.clone(),
                    left: None,
                    right: None,
                    emitted: if index + 1 == names.len() {
                        wire.emitted
                    } else {
                        true
                    },
                    after: names
                        .get(index + 1)
                        .map(|name| name.as_bytes().to_vec())
                        .or_else(|| wire.after.as_ref().map(|name| name.as_bytes().to_vec())),
                });
            }
        }
        Ok(Self {
            left: wire.left.parse().map_err(D::Error::custom)?,
            right: wire.right.parse().map_err(D::Error::custom)?,
            frames,
            results_after: wire.results_after.map(String::into_bytes),
            done: wire.done,
        })
    }
}
