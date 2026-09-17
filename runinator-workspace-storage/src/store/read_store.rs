#[allow(unused_imports)]
use super::*;

pub trait ReadStore: Sync {
    fn info(&self, id: Id) -> Result<ObjectInfo>;
    fn get(&self, id: Id) -> Result<Object>;
    /// Read objects in input order; backends may fetch independent objects concurrently.
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        if ids.len() < PARALLEL_BATCH_MIN {
            return ids.iter().map(|id| self.get(*id)).collect();
        }
        // remote adapters may wait on an async runtime; never lend their io to rayon callers.
        std::thread::scope(|scope| {
            let tasks = ids
                .chunks(ids.len().div_ceil(8))
                .map(|batch| {
                    scope.spawn(move || {
                        batch
                            .iter()
                            .map(|id| self.get(*id))
                            .collect::<Result<Vec<_>>>()
                    })
                })
                .collect::<Vec<_>>();
            let mut objects = Vec::with_capacity(ids.len());
            for task in tasks {
                objects.extend(task.join().map_err(|_| Error::Conflict)??);
            }
            Ok(objects)
        })
    }
    fn contains(&self, id: Id) -> Result<bool> {
        match self.info(id) {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

impl<S: ReadStore + ?Sized> ReadStore for &S {
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        (**self).get_many(ids)
    }
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        (**self).info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        (**self).get(id)
    }
}
