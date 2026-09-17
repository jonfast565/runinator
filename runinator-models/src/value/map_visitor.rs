#[allow(unused_imports)]
use super::*;

pub(super) struct MapVisitor;

impl<'de> Visitor<'de> for MapVisitor {
    type Value = Map;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a json object")
    }

    fn visit_unit<E>(self) -> Result<Map, E> {
        Ok(Map::new())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Map, A::Error> {
        let mut map = Map::new();
        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }
        Ok(map)
    }
}
